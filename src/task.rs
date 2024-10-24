use std::any::Any;
use std::future::Future;
use std::ops::Sub;
use std::pin::Pin;
use std::sync::atomic::AtomicU64;
use std::task::Poll;
use priority::Priority;
use crate::context::TaskLocalFuture;
use crate::hint::Hint;
use crate::observer::{observer_channel, Observer, ObserverNotifier, ObserverSender};
use crate::task_local;

/**
A task identifier.

This is a unique identifier for a task.
*/
#[derive(Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub struct TaskID(u64);

impl<F: Future> From<&Task<F>> for TaskID {
    fn from(task: &Task<F>) -> Self {
        task.task_id
    }
}

impl<F: Future> AsRef<TaskID> for Task<F> {
    fn as_ref(&self) -> &TaskID {
        &self.task_id
    }
}



static TASK_ID: AtomicU64 = AtomicU64::new(0);


/**
A top-level future.

The Task contains information that can be useful to an executor when deciding how to run the future.
*/
#[derive(Debug)]
#[must_use]
pub struct Task<F> where F: Future {
    future: TaskLocalFuture<Priority, TaskLocalFuture<String, F>>,
    hint: Hint,
    poll_after: std::time::Instant,
    task_id: TaskID,
}

/**
A task suitable for spawning.

Executors convert [Task] into this type in order to poll the future.
*/
pub struct SpawnedTask<F,Notifier> where F: Future {
    task: Task<F>,
    sender: ObserverSender<F::Output,Notifier>
}

impl<F: Future, Notifier> SpawnedTask<F,Notifier> {
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

}

task_local! {
    pub static TASK_LABEL: String;
    pub static TASK_PRIORITY: priority::Priority;
}

impl<F: Future> Task<F> {
    pub fn new(label: String, future: F, configuration: Configuration) -> Self where F: Future {
        let apply_label = TASK_LABEL.scope_internal(label, future);
        let apply = TASK_PRIORITY.scope_internal(configuration.priority, apply_label);
        let task_id = TaskID(TASK_ID.fetch_add(1,std::sync::atomic::Ordering::Relaxed));
        assert_ne!(task_id.0, 0, "TaskID overflow");
        Task {
            future: apply,
            hint: configuration.hint,
            poll_after: configuration.poll_after,
            task_id,
        }
    }


    pub fn hint(&self) -> Hint {
        self.hint
    }

    pub fn label(&self) -> String {
        self.future.get_future().get_val(|label| label.clone())
    }

    pub fn priority(&self) -> priority::Priority {
        self.future.get_val(|priority| *priority)
    }

    pub fn poll_after(&self) -> std::time::Instant {
        self.poll_after
    }

    pub fn task_id(&self) -> TaskID {
        self.task_id
    }

    pub fn into_future(self) -> F {
        self.future.into_future().into_future()
    }

    pub fn spawn<Notifier: ObserverNotifier<F::Output>>(self, notify: Option<Notifier>) -> (SpawnedTask<F,Notifier>, Observer<F::Output>) {
        let (sender, receiver) = observer_channel(notify);
        let spawned_task = SpawnedTask {
            task: self,
            sender
        };
        (spawned_task, receiver)
    }

}


impl<F,Notifier> Future for SpawnedTask<F,Notifier> where F: Future, Notifier: ObserverNotifier<F::Output> {
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
        self.future.get_future().get_future()
    }
}

/*
Support AsMut for the underlying future type
 */
impl <F: Future> AsMut<F> for Task<F> {
    fn as_mut(&mut self) -> &mut F {
        self.future.get_future_mut().get_future_mut()
    }
}


#[cfg(test)] mod tests {
    use std::future::Future;
    use crate::task::Task;
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


    }
}