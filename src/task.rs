use std::any::Any;
use std::future::Future;
use std::marker::PhantomData;
use std::ops::Sub;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::task::Poll;
use crate::context::{TaskLocalImmutableFuture};
use crate::hint::Hint;
use crate::observer::{observer_channel, ExecutorNotified, NoNotified, Observer, ObserverNotified, ObserverSender};
use crate::task_local;

/**
A task identifier.

This is a unique identifier for a task.
*/
#[derive(Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub struct TaskID(u64);

impl<F: Future> From<&Task<F>> for TaskID {
    fn from(task: &Task<F>) -> Self {
        task.task_id()
    }
}





static TASK_IDS: AtomicU64 = AtomicU64::new(0);


/**
A top-level future.

The Task contains information that can be useful to an executor when deciding how to run the future.
*/
#[derive(Debug)]
#[must_use]
pub struct Task<F> where F: Future {
    future: TaskLocalImmutableFuture<TaskID, TaskLocalImmutableFuture<InFlightTaskCancellation, TaskLocalImmutableFuture<priority::Priority, TaskLocalImmutableFuture<String, F>>>>,
    hint: Hint,
    poll_after: std::time::Instant,
}

/**
A task suitable for spawning.

Executors convert [Task] into this type in order to poll the future.
*/
#[derive(Debug)]
pub struct SpawnedTask<F,ONotifier,ENotifier> where F: Future {
    task: Task<F>,
    sender: ObserverSender<F::Output,ONotifier>,
    phantom: std::marker::PhantomData<ENotifier>
}

impl<F: Future, ONotifier,ENotifier> SpawnedTask<F,ONotifier,ENotifier> {
    /**
    Access the underlying task.
*/
    pub fn task(&self) -> &Task<F> {
        &self.task
    }

    /**
    Access the underlying task.
*/
    pub fn task_mut(&mut self) -> &mut Task<F> {
        &mut self.task
    }

    pub fn into_task(self) -> Task<F> {
        self.task
    }

}
/**
Provides information about the cancellation status of the current task.
*/
#[derive(Debug)]
pub struct InFlightTaskCancellation(Arc<AtomicBool>);

impl InFlightTaskCancellation {
    //we don't publish this so that we can change implementation later
    pub(crate) fn clone(&self) -> Self {
        InFlightTaskCancellation(self.0.clone())
    }

    pub(crate) fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /**
    Returns true if the task has been cancelled.

    Code inside the task may wish to check this and return early.

    It is not required that anyone check this value.
    */
    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }


}


task_local! {
    /**
    Provides a debugging label to identify the current task.
*/
    pub static const TASK_LABEL: String;
    /**
    Provides a priority for the current task.
     */
    pub static const TASK_PRIORITY: priority::Priority;
    /**
    Provides a mechanism for tasks to determine if they have been cancelled.

    Tasks can use this to determine if they should stop running.
*/
    pub static const IS_CANCELLED: InFlightTaskCancellation;

    /**
    Provides a unique identifier for the current task.
     */
    pub static const TASK_ID: TaskID;
}

impl<F: Future> Task<F> {
    pub fn new(label: String, future: F, configuration: Configuration) -> Self where F: Future {
        let task_id = TaskID(TASK_IDS.fetch_add(1,std::sync::atomic::Ordering::Relaxed));

        let apply_label = TASK_LABEL.scope_internal(label, future);
        let apply_priority = TASK_PRIORITY.scope_internal(configuration.priority, apply_label);
        let apply_cancellation = IS_CANCELLED.scope_internal(InFlightTaskCancellation(Arc::new(AtomicBool::new(false))), apply_priority);
        let apply = TASK_ID.scope_internal(task_id, apply_cancellation);
        assert_ne!(task_id.0, 0, "TaskID overflow");
        Task {
            future: apply,
            hint: configuration.hint,
            poll_after: configuration.poll_after,
        }
    }


    pub fn hint(&self) -> Hint {
        self.hint
    }

    pub fn label(&self) -> String {
        self.future.get_future().get_future().get_future().get_val(|label| label.clone())
    }

    pub fn priority(&self) -> priority::Priority {
        self.future.get_future().get_future().get_val(|priority| *priority)
    }

    pub(crate) fn task_cancellation(&self) -> InFlightTaskCancellation {
        self.future.get_future().get_val(|cancellation| cancellation.clone())
    }

    pub fn poll_after(&self) -> std::time::Instant {
        self.poll_after
    }

    pub fn task_id(&self) -> TaskID {
        self.future.get_val(|task_id| *task_id)
    }

    pub fn into_future(self) -> F {
        self.future.into_future().into_future().into_future().into_future()
    }

    pub fn spawn<ONotifier: ObserverNotified<F::Output>,ENotifier: ExecutorNotified>(self, observer_notifier: Option<ONotifier>,executor_notifier: Option<ENotifier>) -> (SpawnedTask<F,ONotifier,ENotifier>, Observer<F::Output,ENotifier>) {
        let (sender, receiver) = observer_channel(observer_notifier,executor_notifier,self.task_cancellation().clone(),self.task_id());
        let spawned_task = SpawnedTask {
            task: self,
            sender,
            phantom: PhantomData
        };
        (spawned_task, receiver)
    }

}


impl<F,ONotifier,ENotifier> Future for SpawnedTask<F,ONotifier,ENotifier> where F: Future, ONotifier: ObserverNotified<F::Output> {
    type Output = ();

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        assert!(self.task.poll_after <= std::time::Instant::now(), "Conforming executors should not poll tasks before the poll_after time.");
        //destructure
        let (future,sender) = unsafe {
            let unchecked = self.get_unchecked_mut();
            let future = Pin::new_unchecked(&mut unchecked.task.future);
            let sender = Pin::new_unchecked(&mut unchecked.sender);
            (future,sender)
        };

        if sender.observer_cancelled() {
            //we don't really need to notify the observer here.  Also the notifier will run upon drop.
            return Poll::Ready(());
        }
        match future.poll(cx) {
            Poll::Ready(r) => {

                sender.get_mut().send(r);
                Poll::Ready(())
            }
            Poll::Pending => {
                Poll::Pending
            }
        }


    }
}

impl Task<Pin<Box<dyn Future<Output=Box<dyn Any>> + Send + 'static>>> {
    pub fn new_objsafe(label: String, future: Box<dyn Future<Output=Box<dyn Any>> + Send + 'static>, configuration: Configuration) -> Self {
        Self::new(label, Box::into_pin(future), configuration)
    }
}


/**
Information needed to spawn a task.
*/
#[derive(Debug,Clone,PartialEq,Eq,Hash)]
pub struct Configuration {
    hint: Hint,
    priority: priority::Priority,
    poll_after: std::time::Instant,
}

/**
A builder for [Configuration].
*/
pub struct ConfigurationBuilder {
    hint: Option<Hint>,
    priority: Option<priority::Priority>,
    poll_after: Option<std::time::Instant>,
}

impl ConfigurationBuilder {
    pub fn new() -> Self {
        ConfigurationBuilder {
            hint: None,
            priority: None,
            poll_after: None,
        }
    }

    /**
    Provide a hint about the runtime characteristics of the future.
    */

    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }

    /**
    Provide a priority for the future.

    See the [Priority] type for details.
    */
    pub fn priority(mut self, priority: priority::Priority) -> Self {
        self.priority = Some(priority);
        self
    }

    /**
    Provide a time after which the future should be polled.
*/
    pub fn poll_after(mut self, poll_after: std::time::Instant) -> Self {
        self.poll_after = Some(poll_after);
        self
    }

    pub fn build(self) -> Configuration {
        Configuration {
            hint: self.hint.unwrap_or_else(|| Hint::default()),
            priority: self.priority.unwrap_or_else(|| priority::Priority::Unknown),
            poll_after: self.poll_after.unwrap_or_else(|| std::time::Instant::now().sub(std::time::Duration::from_secs(1))),
        }
    }

}

impl Configuration {
    pub fn new(hint: Hint, priority: priority::Priority, poll_after: std::time::Instant) -> Self {
        Configuration {
            hint,
            priority,
            poll_after
        }
    }
}

/* boilerplates

configuration - default

*/
impl Default for Configuration {
    fn default() -> Self {
        Configuration {
            hint: Hint::default(),
            priority: priority::Priority::Unknown,
            poll_after: std::time::Instant::now().sub(std::time::Duration::from_secs(1)),
        }
    }
}

/*
I don't think it makes sense to support Clone on Task.
That eliminates the need for PartialEq, Eq, Hash.  We have ID type for this.

I suppose we could implement Default with a blank task...

 */
#[derive(Debug,Copy,Clone,PartialEq,Eq,Hash,Default)]
pub struct DefaultFuture;
impl Future for DefaultFuture {
    type Output = ();
    fn poll(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        Poll::Ready(())
    }
}
impl Default for Task<DefaultFuture> {
    fn default() -> Self {
        Task::new("".to_string(), DefaultFuture, Configuration::default())
    }
}

/*
Support from for the Future type
 */

impl<F: Future> From<F> for Task<F> {
    fn from(future: F) -> Self {
        Task::new("".to_string(), future, Configuration::default())
    }
}

/*
Support AsRef for the underlying future type
 */

impl<F: Future> AsRef<F> for Task<F> {
    fn as_ref(&self) -> &F {
        self.future.get_future().get_future().get_future().get_future()
    }
}

/*
Support AsMut for the underlying future type
 */
impl <F: Future> AsMut<F> for Task<F> {
    fn as_mut(&mut self) -> &mut F {
        self.future.get_future_mut().get_future_mut().get_future_mut().get_future_mut()
    }
}

/*
Analogously, for spawned task...
 */

impl Default for SpawnedTask<DefaultFuture,NoNotified,NoNotified> {
    fn default() -> Self {
        Task::default().spawn(None,None).0
    }
}

impl<F: Future> From<F> for SpawnedTask<F,NoNotified,NoNotified> {
    fn from(future: F) -> Self {
        Task::from(future).spawn(None,None).0
    }
}


impl<F: Future> AsRef<F> for SpawnedTask<F,NoNotified,NoNotified> {
    fn as_ref(&self) -> &F {
        self.task().as_ref()
    }
}

impl<F: Future> AsMut<F> for SpawnedTask<F,NoNotified,NoNotified> {
    fn as_mut(&mut self) -> &mut F {
        self.task_mut().as_mut()
    }
}



#[cfg(test)] mod tests {
    use std::future::Future;
    use crate::observer::NoNotified;
    use crate::task::{SpawnedTask, Task};
    use crate::task_local;
    #[test] fn test_send() {
        task_local!(
            static FOO: u32;
        );

        let scoped = FOO.scope(42, async {});

        fn assert_send<T: Send>(_: T) {}
        assert_send(scoped);
    }

    #[test] fn test_send_task() {
        #[allow(unused)]
        fn task_check<F: Future + Send>(task: Task<F>) {
            fn assert_send<T: Send>(_: T) {}
            assert_send(task);


        }
        #[allow(unused)]
        fn task_check_sync<F: Future + Sync>(task: Task<F>) {
            fn assert_sync<T: Sync>(_: T) {}
            assert_sync(task);
        }
        #[allow(unused)]
        fn task_check_unpin<F: Future + Unpin>(task: Task<F>) {
            fn assert_unpin<T: Unpin>(_: T) {}
            assert_unpin(task);
        }

        #[allow(unused)]
        fn spawn_check<F: Future + Send>(task: Task<F>) where F::Output: Send {
            let spawned: SpawnedTask<F,NoNotified,NoNotified> = task.spawn(None,None).0;
            fn assert_send<T: Send>(_: T) {}
            assert_send(spawned);
        }

        #[allow(unused)]
        fn spawn_check_sync<F: Future + Sync>(task: Task<F>) where F::Output: Send {
            let spawned: SpawnedTask<F,NoNotified,NoNotified> = task.spawn(None,None).0;
            fn assert_sync<T: Sync>(_: T) {}
            assert_sync(spawned);
        }

        #[allow(unused)]
        fn spawn_check_unpin<F: Future + Unpin>(task: Task<F>) {
            let spawned: SpawnedTask<F,NoNotified,NoNotified> = task.spawn(None,None).0;
            fn assert_unpin<T: Unpin>(_: T) {}
            assert_unpin(spawned);
        }


    }
}