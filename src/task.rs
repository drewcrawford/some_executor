//SPDX-License-Identifier: MIT OR Apache-2.0

use std::any::Any;
use std::cell::RefCell;
use std::fmt::Debug;
use std::future::{Future};
use std::marker::PhantomData;
use std::ops::{Sub};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::task::{Context, Poll};
use crate::context::{TaskLocalImmutableFuture};
use crate::hint::Hint;
use crate::observer::{observer_channel, ExecutorNotified, NoNotified, Observer, ObserverNotified, ObserverSender};
use crate::{task_local, DynExecutor, SomeLocalExecutor, Priority, SomeExecutor};
use crate::local::UnsafeErasedLocalExecutor;

/**
A task identifier.

This is a unique identifier for a task.
*/
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskID(u64);

impl<F: Future, N> From<&Task<F, N>> for TaskID {
    fn from(task: &Task<F, N>) -> Self {
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
pub struct Task<F, N>
where
    F: Future,
{
    future: F,
    hint: Hint,
    label: String,
    poll_after: std::time::Instant,
    notifier: Option<N>,
    priority: Priority,
    task_id: TaskID,
}

/**
A task suitable for spawning.

Executors convert [Task] into this type in order to poll the future.
*/
#[derive(Debug)]
pub struct SpawnedTask<F, ONotifier, Executor>
where
    F: Future,
{
    task: F,
    sender: ObserverSender<F::Output, ONotifier>,
    phantom: PhantomData<Executor>,
    poll_after: std::time::Instant,
    hint: Hint,
    //these task_local properties optional so we can take/replace them
    label: Option<String>,
    cancellation: Option<InFlightTaskCancellation>,
    executor: Option<Box<dyn SomeExecutor<ExecutorNotifier=NoNotified>>>,
    priority: Priority, //copy,so no need to repair/replace them, boring
    task_id: TaskID, //boring
}

/**
A task suitable for spawning.

Executors convert [Task] into this type in order to poll the future.

# Design note

[SpawnedTask] has an embedded copy of the executor. This is fine for that type, since shared executors
necessarily involve some nice owned type, like a channel or Arc<Mutex>.

[LocalExecutor] is quite different, and may be implemented as a reference to a local executor.  A few issues
with this, the big one is that we want to spawn with `&mut Executor`, but if tasks embed some kind of &Executor,
we can't do that.

Instead what we need to do is inject the executor on each poll, which means that the task itself cannot be a future.

*/
#[derive(Debug)]
pub struct SpawnedLocalTask<F, ONotifier, Executor>
where
    F: Future,
{
    task: F,
    sender: ObserverSender<F::Output, ONotifier>,
    poll_after: std::time::Instant,
    executor: PhantomData<Executor>,
    label: String,
    hint: Hint,
    priority: Priority,
    task_id: TaskID,
}


impl<F: Future, ONotifier, ENotifier> SpawnedTask<F, ONotifier, ENotifier> {
    pub fn hint(&self) -> Hint {
        self.hint
    }

    pub fn label(&self) -> &str {
        self.label.as_ref().expect("Future is polling")
    }

    pub fn priority(&self) -> priority::Priority {
        self.priority
    }

    // pub(crate) fn task_cancellation(&self) -> InFlightTaskCancellation {
    //     self.task.get_future().get_future().get_val(|cancellation| cancellation.clone())
    // }

    pub fn poll_after(&self) -> std::time::Instant {
        self.poll_after
    }

    pub fn task_id(&self) -> TaskID {
        self.task_id
    }

    pub fn into_future(self) -> F {
        self.task
    }
}

impl<'executor, F: Future, ONotifier, Executor> SpawnedLocalTask<F, ONotifier, Executor> {
    pub fn hint(&self) -> Hint {
        self.hint
    }

    pub fn label(&self) -> &str {
        self.label.as_ref()
    }

    pub fn priority(&self) -> priority::Priority {
        self.priority
    }

    // pub(crate) fn task_cancellation(&self) -> InFlightTaskCancellation {
    //     self.task.get_future().get_future().get_val(|cancellation| cancellation.clone())
    // }

    pub fn poll_after(&self) -> std::time::Instant {
        self.poll_after
    }

    pub fn task_id(&self) -> TaskID {
        self.task_id
    }

    pub fn into_future(self) -> F {
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

    /**
    Provides an executor local to the current task.
    */
    pub static const TASK_EXECUTOR: Option<Box<DynExecutor>>;
}

thread_local! {
    static TASK_LOCAL_EXECUTOR: RefCell<Option<Box<dyn SomeLocalExecutor<'static, ExecutorNotifier=Box<dyn ExecutorNotified>>>>> = RefCell::new(None);
}

impl<F: Future, N> Task<F, N> {
    /**
    Creates a new task.

    # Parameters
    - `label`: A human-readable label for the task.
    - `future`: The future to run.
    - `configuration`: Configuration for the task.
    - `notifier`: An observer to notify when the task completes.  If there is no notifier, consider using [Self::without_notifications] instead.
*/
    pub fn with_notifications(label: String, future: F, configuration: Configuration, notifier: Option<N>) -> Self
    where
        F: Future,
    {
        let task_id = TaskID(TASK_IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed));

        assert_ne!(task_id.0, u64::MAX, "TaskID overflow");
        Task {
            label,
            future,
            hint: configuration.hint,
            poll_after: configuration.poll_after,
            priority: configuration.priority,
            notifier,
            task_id,
        }
    }

    pub fn hint(&self) -> Hint {
        self.hint
    }

    pub fn label(&self) -> &str {
        self.label.as_ref()
    }

    pub fn priority(&self) -> priority::Priority {
        self.priority
    }



    pub fn poll_after(&self) -> std::time::Instant {
        self.poll_after
    }

    pub fn task_id(&self) -> TaskID {
        self.task_id
    }

    pub fn into_future(self) -> F {
        self.future
    }

    /**
    Spawns the task onto the executor.

    When using this method, the TASK_LOCAL_EXECUTOR will be set to None.
    To spawn a task onto a local executor instead, use [Task::spawn_local].
    */
    pub fn spawn<Executor: SomeExecutor>(mut self, executor: &mut Executor) -> (SpawnedTask<F, N, Executor::ExecutorNotifier>, Observer<F::Output, Executor::ExecutorNotifier>) {
        let cancellation = InFlightTaskCancellation::default();
        let some_notifier: Option<Executor::ExecutorNotifier> = executor.executor_notifier();
        let task_id = self.task_id();
        let (sender, receiver) = observer_channel(self.notifier.take(), some_notifier, cancellation.clone(), task_id);
        let boxed_executor = executor.clone_box();
        let spawned_task = SpawnedTask {
            task: self.future,
            sender,
            phantom: PhantomData,
            poll_after: self.poll_after,
            hint: self.hint,
            label: Some(self.label),
            priority: self.priority,
            task_id,
            cancellation: Some(cancellation),
            executor: Some(boxed_executor),
        };
        (spawned_task, receiver)
    }

    /**
    Spawns the task onto a local executor
    */
    pub fn spawn_local<'executor, Executor: SomeLocalExecutor<'executor>>(mut self, executor: &mut Executor) -> (SpawnedLocalTask<F, N, Executor::ExecutorNotifier>, Observer<F::Output, Executor::ExecutorNotifier>) {
        let cancellation = InFlightTaskCancellation::default();
        let task_id = self.task_id();
        let (sender, receiver) = observer_channel(self.notifier.take(), executor.executor_notifier(), cancellation, task_id);
        let spawned_task = SpawnedLocalTask {
            task: self.future,
            sender,
            executor: PhantomData,
            poll_after: self.poll_after,
            hint: self.hint,
            priority: self.priority,
            label: self.label,
            task_id,
        };
        (spawned_task, receiver)
    }

    pub fn spawn_objsafe(mut self, executor: &mut (dyn SomeExecutor<ExecutorNotifier=NoNotified> + 'static)) -> (SpawnedTask<F, N, Box<dyn ExecutorNotified + Send>>, Observer<F::Output, Box<dyn ExecutorNotified + Send>>) {
        let cancellation = InFlightTaskCancellation::default();
        let boxed_executor_notifier = executor.executor_notifier().map(|n| Box::new(n) as Box<dyn ExecutorNotified + Send>);
        let boxed_executor = executor.clone_box();
        let (sender, receiver) = observer_channel(self.notifier.take(), boxed_executor_notifier, cancellation.clone(), self.task_id);
        let spawned_task = SpawnedTask {
            task: self.future,
            sender,
            phantom: PhantomData,
            poll_after: self.poll_after,
            hint: self.hint,
            label: Some(self.label),
            priority: self.priority,
            task_id: self.task_id,
            cancellation: Some(cancellation),
            executor: Some(boxed_executor),
        };
        (spawned_task, receiver)
    }

    /**
    Spawns the task onto a local executor.

    For this to work, we have to erase the output type to dyn Any.
    */
    pub fn spawn_local_objsafe(mut self, executor: &mut (dyn SomeLocalExecutor<ExecutorNotifier=NoNotified>)) -> (SpawnedLocalTask<F, N, Box<dyn ExecutorNotified>>, Observer<F::Output, Box<dyn ExecutorNotified>>) {
        let cancellation = InFlightTaskCancellation::default();
        let task_id = self.task_id();
        let boxed_executor_notifier = executor.executor_notifier().map(|n| Box::new(n) as Box<dyn ExecutorNotified>);
        let (sender, receiver) = observer_channel(self.notifier.take(), boxed_executor_notifier, cancellation, task_id);
        let spawned_task = SpawnedLocalTask {
            task: self.future,
            sender,
            poll_after: self.poll_after,
            hint: self.hint,
            priority: self.priority,
            executor: PhantomData,
            label: self.label,
            task_id,
        };
        (spawned_task, receiver)
    }
}

impl<F: Future> Task<F, NoNotified> {
    /**
    Spawns a task, without performing inline notification.

    Use this constructor when there are no cancellation notifications desired.

    # Parameters
    - `label`: A human-readable label for the task.
    - `future`: The future to run.
    - `configuration`: Configuration for the task.

    # Details

    Use of this function is equivalent to calling [Task::with_notifications] with a None notifier.

    This function avoids the need to specify the type parameter to [Task].
    */
    pub fn without_notifications(label: String, future: F, configuration: Configuration) -> Self {
        Task::with_notifications(label, future, configuration, None)
    }
}


impl<F, ONotifier, ENotifier> Future for SpawnedTask<F, ONotifier, ENotifier>
where
    F: Future,
    ONotifier: ObserverNotified<F::Output>,
{
    type Output = ();

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        assert!(self.poll_after <= std::time::Instant::now(), "Conforming executors should not poll tasks before the poll_after time.");
        //destructure
        let (future, sender, mut label, priority,
            cancellation, task_id,executor) = unsafe {
            let unchecked = self.get_unchecked_mut();
            let future = Pin::new_unchecked(&mut unchecked.task);
            let sender = Pin::new_unchecked(&mut unchecked.sender);
            let label = Pin::new_unchecked(&mut unchecked.label);
            let priority = Pin::new_unchecked(&mut unchecked.priority);
            let cancellation = Pin::new_unchecked(&mut unchecked.cancellation);
            let task_id = unchecked.task_id;
            let executor = Pin::new_unchecked(&mut unchecked.executor);
            (future, sender, label, priority, cancellation, task_id, executor)
        };

        if sender.observer_cancelled() {
            //we don't really need to notify the observer here.  Also the notifier will run upon drop.
            return Poll::Ready(());
        }
        //before poll, we need to set our properties
        let label = label.get_mut();
        let cancellation = cancellation.get_mut();
        let executor = executor.get_mut();
        unsafe {
            TASK_LABEL.with_mut(|l| {
                *l = Some(label.take().expect("Label not set (is task being polled already?)"));
            });
            IS_CANCELLED.with_mut(|c| {
                *c = Some(cancellation.take().expect("Cancellation not set (is task being polled already?)"));
            });
            TASK_PRIORITY.with_mut(|p| {
                *p = Some(*priority.get_mut());
            });
            TASK_ID.with_mut(|i| {
                *i = Some(task_id);
            });
            TASK_EXECUTOR.with_mut(|e| {
                *e = Some(Some(executor.take().expect("Executor not set (is task being polled already?)")));
            });
            TASK_LOCAL_EXECUTOR.with_borrow_mut(|e| {
               *e = None;
            });


        }
        let r = future.poll(cx);
        //after poll, we need to set our properties
        unsafe {
            TASK_LABEL.with_mut(|l| {
                let read_label = l.take().expect("Label not set");
                *label = Some(read_label);
            });
            IS_CANCELLED.with_mut(|c| {
                let read_cancellation = c.take().expect("Cancellation not set");
                *cancellation = Some(read_cancellation);
            });
            TASK_PRIORITY.with_mut(|p| {
                *p = None;
            });
            TASK_ID.with_mut(|i| {
                *i = None;
            });
            TASK_EXECUTOR.with_mut(|e| {
                let read_executor = e.take().expect("Executor not set").expect("Executor not set");
                *executor = Some(read_executor);
            });
            TASK_LOCAL_EXECUTOR.with_borrow_mut(|e| {
                *e = None;
            });
        }
        match r {
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

impl<'executor, F, ONotifier, Executor: SomeLocalExecutor<'executor>> SpawnedLocalTask<F, ONotifier, Executor>
where
    F: Future,
    ONotifier: ObserverNotified<F::Output>,
{
    //I can't believe it's not future

    pub fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, executor: &'executor mut Executor) -> std::task::Poll<()> {
        assert!(self.poll_after <= std::time::Instant::now(), "Conforming executors should not poll tasks before the poll_after time.");
        //destructure
        let (future, sender) = unsafe {
            let unchecked = self.get_unchecked_mut();
            let future = Pin::new_unchecked(&mut unchecked.task);
            let sender = Pin::new_unchecked(&mut unchecked.sender);
            (future, sender)
        };
        //set local executor
        let mut erased_value_executor = Box::new(crate::local::SomeLocalExecutorErasingNotifier::new(executor)) as Box<dyn SomeLocalExecutor<ExecutorNotifier=Box<dyn ExecutorNotified>> + '_>;
        let erased_value_executor_ref = Box::as_mut(&mut erased_value_executor);

        //I solemnly swear I'm up to no good
        unsafe {
            let erased_unsafe_executor = UnsafeErasedLocalExecutor::new(erased_value_executor_ref);
            TASK_LOCAL_EXECUTOR.with(|e| {
                e.borrow_mut().replace(Box::new(erased_unsafe_executor));
            });
        }

        if sender.observer_cancelled() {
            //we don't really need to notify the observer here.  Also the notifier will run upon drop.
            return Poll::Ready(());
        }
        //perform poll
        let f = future.poll(cx);
        //clear local executor
        TASK_LOCAL_EXECUTOR.with(|e| {
            e.borrow_mut().take().expect("Local executor not set");
        });
        match f {
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

impl Task<Pin<Box<dyn Future<Output=Box<dyn Any + Send + 'static>> + Send + 'static>>, Box<dyn ObserverNotified<Box<dyn Any + Send>> + Send>> {
    pub fn new_objsafe(label: String, future: Box<dyn Future<Output=Box<dyn Any + Send + 'static>> + Send + 'static>, configuration: Configuration, notifier: Option<Box<dyn ObserverNotified<Box<dyn Any + Send>> + Send>>) -> Self {
        Self::with_notifications(label, Box::into_pin(future), configuration, notifier)
    }
}


/**
Information needed to spawn a task.
*/
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Configuration {
    hint: Hint,
    priority: priority::Priority,
    poll_after: std::time::Instant,
}

/**
A builder for [Configuration].
*/
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ConfigurationBuilder {
    hint: Option<Hint>,
    priority: Option<Priority>,
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
            poll_after,
        }
    }
}

/**
ObjSafe type-erased wrapper for [SpawnedLocalTask].
*/
pub trait DynLocalSpawnedTask {
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>, executor: &mut dyn SomeLocalExecutor<ExecutorNotifier=NoNotified>) -> std::task::Poll<()>;
    fn poll_after(&self) -> std::time::Instant;
    fn label(&self) -> &str;

    fn hint(&self) -> Hint;
    fn priority(&self) -> priority::Priority;
}

impl<'executor, F, ONotifier, Executor> DynLocalSpawnedTask for SpawnedLocalTask<F, ONotifier, Executor>
where
    F: Future,
{
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>, executor: &mut dyn SomeLocalExecutor<ExecutorNotifier=NoNotified>) -> Poll<()> {
        self.poll(cx, executor)
    }

    fn poll_after(&self) -> std::time::Instant {
        self.poll_after
    }
    fn label(&self) -> &str {
        self.label()
    }

    fn hint(&self) -> Hint {
        self.hint
    }
    fn priority(&self) -> priority::Priority {
        self.priority()
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
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
pub struct DefaultFuture;
impl Future for DefaultFuture {
    type Output = ();
    fn poll(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        Poll::Ready(())
    }
}
impl Default for Task<DefaultFuture, NoNotified> {
    fn default() -> Self {
        Task::with_notifications("".to_string(), DefaultFuture, Configuration::default(), None)
    }
}

/*
Support from for the Future type
 */

impl<F: Future, N> From<F> for Task<F, N> {
    fn from(future: F) -> Self {
        Task::with_notifications("".to_string(), future, Configuration::default(), None)
    }
}

/*
Support AsRef for the underlying future type
 */

impl<F: Future, N> AsRef<F> for Task<F, N> {
    fn as_ref(&self) -> &F {
        &self.future
    }
}

/*
Support AsMut for the underlying future type
 */
impl<F: Future, N> AsMut<F> for Task<F, N> {
    fn as_mut(&mut self) -> &mut F {
        &mut self.future
    }
}

/*
Analogously, for spawned task...
 */


impl<F: Future> AsRef<F> for SpawnedTask<F, NoNotified, NoNotified> {
    fn as_ref(&self) -> &F {
        &self.task
    }
}

impl<F: Future> AsMut<F> for SpawnedTask<F, NoNotified, NoNotified> {
    fn as_mut(&mut self) -> &mut F {
        &mut self.task
    }
}

impl<F: Future> AsRef<F> for SpawnedLocalTask<F, NoNotified, NoNotified> {
    fn as_ref(&self) -> &F {
        &self.task
    }
}

impl<F: Future> AsMut<F> for SpawnedLocalTask<F, NoNotified, NoNotified> {
    fn as_mut(&mut self) -> &mut F {
        &mut self.task
    }
}

/*
InFlightTaskCancellation
- don't want to publish clone right now.  Eliminates Copy,Eq,Hash, etc.

Default is possible I suppose
 */

impl Default for InFlightTaskCancellation {
    fn default() -> Self {
        InFlightTaskCancellation(Arc::new(AtomicBool::new(false)))
    }
}

impl From<bool> for InFlightTaskCancellation {
    fn from(value: bool) -> Self {
        InFlightTaskCancellation(Arc::new(AtomicBool::new(value)))
    }
}

impl Into<bool> for InFlightTaskCancellation {
    fn into(self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}


#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::future::Future;
    use std::pin::Pin;
    use crate::observer::{ExecutorNotified, NoNotified, Observer, ObserverNotified};
    use crate::task::{DynLocalSpawnedTask, SpawnedTask, Task};
    use crate::{task_local, SomeExecutor, SomeLocalExecutor};

    #[test] fn test_create_task() {
        let task: Task<_,NoNotified> = Task::with_notifications("test".to_string(), async {}, Default::default(), None);
        assert_eq!(task.label(), "test");
    }

    #[test] fn test_create_no_notify() {
        let t = Task::without_notifications("test".to_string(), async {}, Default::default());
        assert_eq!(t.label(), "test");
    }
    #[test]
    fn test_send() {
        task_local!(
            static FOO: u32;
        );

        let scoped = FOO.scope(42, async {});

        fn assert_send<T: Send>(_: T) {}
        assert_send(scoped);
    }

    #[test]
    fn test_send_task() {
        #[allow(unused)]
        fn task_check<F: Future + Send, N: Send>(task: Task<F, N>) {
            fn assert_send<T: Send>(_: T) {}
            assert_send(task);
        }
        #[allow(unused)]
        fn task_check_sync<F: Future + Sync, N: Sync>(task: Task<F, N>) {
            fn assert_sync<T: Sync>(_: T) {}
            assert_sync(task);
        }
        #[allow(unused)]
        fn task_check_unpin<F: Future + Unpin, N: Unpin>(task: Task<F, N>) {
            fn assert_unpin<T: Unpin>(_: T) {}
            assert_unpin(task);
        }

        #[allow(unused)]
        fn spawn_check<F: Future + Send, E: SomeExecutor>(task: Task<F, NoNotified>, exec: &mut E)
        where
            F::Output: Send,
            E::ExecutorNotifier: Send,
        {
            let spawned: SpawnedTask<F, NoNotified, E::ExecutorNotifier> = task.spawn(exec).0;
            fn assert_send<T: Send>(_: T) {}
            assert_send(spawned);
        }

        #[allow(unused)]
        fn spawn_check_sync<F: Future + Sync, E: SomeExecutor>(task: Task<F, NoNotified>, exec: &mut E)
        where
            F::Output: Send,
            E::ExecutorNotifier: Sync,
        {
            let spawned: SpawnedTask<F, NoNotified, E::ExecutorNotifier> = task.spawn(exec).0;
            fn assert_sync<T: Sync>(_: T) {}
            assert_sync(spawned);
        }

        #[allow(unused)]
        fn spawn_check_unpin<F: Future + Unpin, E: SomeExecutor>(task: Task<F, NoNotified>, exec: &mut E)
        where
            E::ExecutorNotifier: Unpin,
        {
            let spawned: SpawnedTask<F, NoNotified, E::ExecutorNotifier> = task.spawn(exec).0;
            fn assert_unpin<T: Unpin>(_: T) {}
            assert_unpin(spawned);
        }
    }



    #[test]
    fn test_local_executor() {

        struct ExLocalExecutor<'future>(Vec<Pin<Box<dyn DynLocalSpawnedTask + 'future>>>);

        impl<'existing_tasks,'new_task> SomeLocalExecutor<'new_task> for ExLocalExecutor<'existing_tasks> where 'new_task: 'existing_tasks {
            type ExecutorNotifier = NoNotified;

            fn spawn_local<'a, F: Future, Notifier: ObserverNotified<F::Output>>(&'a mut self, task: Task<F, Notifier>) -> Observer<F::Output, Self::ExecutorNotifier>
            where
                Self: Sized,
                F: 'new_task,
            /* I am a little uncertain whether this is really required */
                <F as Future>::Output: Unpin
            {
                let (spawn,observer)  = task.spawn_local(self);
                let pinned_spawn = Box::pin(spawn);
                self.0.push(pinned_spawn);
                observer
            }

            fn spawn_local_async<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F, Notifier>) -> impl Future<Output=Observer<F::Output, Self::ExecutorNotifier>>
            where
                Self: Sized,
                F: 'new_task,
            {
                async {
                    let (spawn,observer)  = task.spawn_local(self);
                    let pinned_spawn = Box::pin(spawn);
                    self.0.push(pinned_spawn);
                    observer
                }
            }

            fn spawn_local_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Observer<Box<dyn Any>, Box<dyn ExecutorNotified>> {
                let (spawn, observer) = task.spawn_local_objsafe(self);
                let pinned_spawn = Box::pin(spawn);
                self.0.push(pinned_spawn);
                observer
            }

            fn spawn_local_objsafe_async<'s>(&'s mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Future<Output=Observer<Box<dyn Any>, Box<dyn ExecutorNotified>>> + 's> {
                Box::new(async {
                    let (spawn, observer) = task.spawn_local_objsafe(self);
                    let pinned_spawn = Box::pin(spawn);
                    self.0.push(pinned_spawn);
                    observer
                })
            }



            fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
                todo!()
            }
        }
    }
}