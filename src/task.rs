//SPDX-License-Identifier: MIT OR Apache-2.0

use crate::dyn_observer_notified::ObserverNotifiedErased;
use crate::hint::Hint;
use crate::local::UnsafeErasedLocalExecutor;
use crate::observer::{
    observer_channel, ExecutorNotified, ObserverNotified, ObserverSender, TypedObserver,
};
use crate::{task_local, DynExecutor, Priority, SomeExecutor, SomeLocalExecutor};
use std::any::Any;
use std::cell::RefCell;
use std::convert::Infallible;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use std::task::{Context, Poll};

/**
A task identifier.

This is a unique identifier for a task.
*/
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskID(u64);

impl TaskID {
    /**
    Creates a task ID from an opaque u64.

    The only valid value comes from a previous call to [TaskID::to_u64].
    */
    pub fn from_u64(id: u64) -> Self {
        TaskID(id)
    }

    /**
    Converts the task ID to an opaque u64.

    The only valid use of this value is to pass it to [TaskID::from_u64].
    */
    pub fn to_u64(self) -> u64 {
        self.0
    }
}

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
    poll_after: crate::sys::Instant,
    notifier: Option<N>,
    priority: Priority,
    task_id: TaskID,
}

/**
A task suitable for spawning.

Executors convert [Task] into this type in order to poll the future.
*/
pub struct SpawnedTask<F, ONotifier, Executor>
where
    F: Future,
{
    task: F,
    sender: ObserverSender<F::Output, ONotifier>,
    phantom: PhantomData<Executor>,
    poll_after: crate::sys::Instant,
    hint: Hint,
    //these task_local properties optional so we can take/replace them
    label: Option<String>,
    cancellation: Option<InFlightTaskCancellation>,
    executor: Option<Box<dyn SomeExecutor<ExecutorNotifier = Infallible>>>,
    priority: Priority, //copy,so no need to repair/replace them, boring
    task_id: TaskID,    //boring
}

/**
A task suitable for spawning.

Executors convert [Task] into this type in order to poll the future.

# Design note

[SpawnedTask] has an embedded copy of the executor. This is fine for that type, since shared executors
necessarily involve some nice owned type, like a channel or `Arc<Mutex>`.

[SomeLocalExecutor] is quite different, and may be implemented as a reference to a local executor.  A few issues
with this, the big one is that we want to spawn with `&mut Executor`, but if tasks embed some kind of &Executor,
we can't do that.

Instead what we need to do is inject the executor on each poll, which means that the task itself cannot be a future.

*/
pub struct SpawnedLocalTask<F, ONotifier, Executor>
where
    F: Future,
{
    task: F,
    sender: ObserverSender<F::Output, ONotifier>,
    poll_after: crate::sys::Instant,
    executor: PhantomData<Executor>,
    //these task-local properties are optional so we can move them in and out
    label: Option<String>,
    cancellation: Option<InFlightTaskCancellation>,
    //copy-safe task-local properties
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

    pub fn poll_after(&self) -> crate::sys::Instant {
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
        self.label.as_ref().expect("Future is polling")
    }

    pub fn priority(&self) -> priority::Priority {
        self.priority
    }

    // pub(crate) fn task_cancellation(&self) -> InFlightTaskCancellation {
    //     self.task.get_future().get_future().get_val(|cancellation| cancellation.clone())
    // }

    pub fn poll_after(&self) -> crate::sys::Instant {
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
    pub static TASK_LOCAL_EXECUTOR: RefCell<Option<Box<dyn SomeLocalExecutor<'static, ExecutorNotifier=Box<dyn ExecutorNotified>>>>> = RefCell::new(None);
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
    pub fn with_notifications(
        label: String,
        future: F,
        configuration: Configuration,
        notifier: Option<N>,
    ) -> Self
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

    /**
    The time after which the task should be polled.

    Executors must not poll the task before this time.  An executor may choose to implement this in a variety of
    ways, such as using a timer, sleeping the thread, etc.
    */
    pub fn poll_after(&self) -> crate::sys::Instant {
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
    pub fn spawn<Executor: SomeExecutor>(
        mut self,
        executor: &mut Executor,
    ) -> (
        SpawnedTask<F, N, Executor>,
        TypedObserver<F::Output, Executor::ExecutorNotifier>,
    ) {
        let cancellation = InFlightTaskCancellation::default();
        let some_notifier: Option<Executor::ExecutorNotifier> = executor.executor_notifier();
        let task_id = self.task_id();
        let (sender, receiver) = observer_channel(
            self.notifier.take(),
            some_notifier,
            cancellation.clone(),
            task_id,
        );
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
    pub fn spawn_local<'executor, Executor: SomeLocalExecutor<'executor>>(
        mut self,
        executor: &mut Executor,
    ) -> (
        SpawnedLocalTask<F, N, Executor>,
        TypedObserver<F::Output, Executor::ExecutorNotifier>,
    ) {
        let cancellation = InFlightTaskCancellation::default();
        let task_id = self.task_id();
        let (sender, receiver) = observer_channel(
            self.notifier.take(),
            executor.executor_notifier(),
            cancellation.clone(),
            task_id,
        );
        let spawned_task = SpawnedLocalTask {
            task: self.future,
            sender,
            executor: PhantomData,
            poll_after: self.poll_after,
            hint: self.hint,
            priority: self.priority,
            label: Some(self.label),
            task_id,
            cancellation: Some(cancellation),
        };
        (spawned_task, receiver)
    }
    /**
    Spawns the task onto a local executor.

    # Objsafe

    A word on exactly what 'objsafe' means in this context.  Objsafe means that whoever is spawning the task,
    doesn't know which executor they are using, so they spawn onto an objsafe executor via the objsafe methods.

    This has two implications.  First, we need to hide the executor type from the spawner.  However, we don't need
    to hide it from the *executor*, since the executor knows what it is.  Accordingly, this information is erased
    with respect to types sent to the spawner, and not erased with respect to types sent
    to the executor.

    Second, the objsafe spawn method cannot have any generics.  Therefore, the future type is erased (boxed) and worse,
    the output type is erased as well.  Accordingly we do not know what it is.
    */

    pub fn spawn_objsafe<Executor: SomeExecutor>(
        mut self,
        executor: &mut Executor,
    ) -> (
        SpawnedTask<F, N, Executor>,
        TypedObserver<F::Output, Box<dyn ExecutorNotified + Send>>,
    ) {
        let cancellation = InFlightTaskCancellation::default();
        let boxed_executor_notifier = executor
            .executor_notifier()
            .map(|n| Box::new(n) as Box<dyn ExecutorNotified + Send>);
        let boxed_executor = executor.clone_box();
        let (sender, receiver) = observer_channel(
            self.notifier.take(),
            boxed_executor_notifier,
            cancellation.clone(),
            self.task_id,
        );
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

    # Objsafe

    A word on exactly what 'objsafe' means in this context.  Objsafe means that whoever is spawning the task,
    doesn't know which executor they are using, so they spawn onto an objsafe executor via the objsafe methods.

    This has two implications.  First, we need to hide the executor type from the spawner.  However, we don't need
    to hide it from the *executor*, since the executor knows what it is.  Accordingly this information is erased
    with respect to types sent to the spawner, and not erased with respect to types sent
    to the executor.

    Second, the objsafe spawn method cannot have any generics.  Therefore, the future type is erased (boxed) and worse,
    the output type is erased as well.  Accordingly we do not know what it is.
    */
    pub fn spawn_local_objsafe<'executor, Executor: SomeLocalExecutor<'executor>>(
        mut self,
        executor: &mut Executor,
    ) -> (
        SpawnedLocalTask<F, N, Executor>,
        TypedObserver<F::Output, Box<dyn ExecutorNotified>>,
    ) {
        let cancellation = InFlightTaskCancellation::default();
        let task_id = self.task_id();

        let boxed_executor_notifier = executor
            .executor_notifier()
            .map(|n| Box::new(n) as Box<dyn ExecutorNotified>);
        let (sender, receiver) = observer_channel(
            self.notifier.take(),
            boxed_executor_notifier,
            cancellation.clone(),
            task_id,
        );
        let spawned_task = SpawnedLocalTask {
            task: self.future,
            sender,
            poll_after: self.poll_after,
            hint: self.hint,
            priority: self.priority,
            executor: PhantomData,
            label: Some(self.label),
            task_id,
            cancellation: Some(cancellation),
        };
        (spawned_task, receiver)
    }

    /**
    Converts this task into one suitable for spawn_objsafe
    */
    pub fn into_objsafe(
        self,
    ) -> Task<
        Pin<Box<dyn Future<Output = Box<dyn Any + 'static + Send>> + 'static + Send>>,
        Box<dyn ObserverNotified<dyn Any + Send> + Send>,
    >
    where
        N: ObserverNotified<F::Output> + Send,
        F::Output: Send + 'static + Unpin,
        F: Send + 'static,
    {
        let notifier = self.notifier.map(|n| {
            Box::new(ObserverNotifiedErased::new(n))
                as Box<dyn ObserverNotified<dyn Any + Send> + Send>
        });
        Task::new_objsafe(
            self.label,
            Box::new(async move { Box::new(self.future.await) as Box<dyn Any + Send + 'static> }),
            Configuration::new(self.hint, self.priority, self.poll_after),
            notifier,
        )
    }
}

//infalliable notification methods

impl<F: Future> Task<F, Infallible> {
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

fn common_poll<'l, F, N, L>(
    future: Pin<&mut F>,
    sender: &mut ObserverSender<F::Output, N>,
    label: &mut Option<String>,
    cancellation: &mut Option<InFlightTaskCancellation>,
    executor: &mut Option<Box<dyn SomeExecutor<ExecutorNotifier = Infallible>>>,
    local_executor: Option<&mut L>,
    priority: Priority,
    task_id: TaskID,
    poll_after: crate::sys::Instant,
    cx: &mut Context,
) -> std::task::Poll<()>
where
    F: Future,
    N: ObserverNotified<F::Output>,
    L: SomeLocalExecutor<'l>,
{
    assert!(
        poll_after <= crate::sys::Instant::now(),
        "Conforming executors should not poll tasks before the poll_after time."
    );
    if sender.observer_cancelled() {
        //we don't really need to notify the observer here.  Also the notifier will run upon drop.
        return Poll::Ready(());
    }
    //before poll, we need to set our properties
    unsafe {
        TASK_LABEL.with_mut(|l| {
            *l = Some(
                label
                    .take()
                    .expect("Label not set (is task being polled already?)"),
            );
        });
        IS_CANCELLED.with_mut(|c| {
            *c = Some(
                cancellation
                    .take()
                    .expect("Cancellation not set (is task being polled already?)"),
            );
        });
        TASK_PRIORITY.with_mut(|p| {
            *p = Some(priority);
        });
        TASK_ID.with_mut(|i| {
            *i = Some(task_id);
        });
        TASK_EXECUTOR.with_mut(|e| {
            *e = Some(executor.take());
        });
        if let Some(local_executor) = local_executor {
            let mut erased_value_executor = Box::new(
                crate::local::SomeLocalExecutorErasingNotifier::new(local_executor),
            )
                as Box<dyn SomeLocalExecutor<ExecutorNotifier = Box<dyn ExecutorNotified>> + '_>;
            let erased_value_executor_ref = Box::as_mut(&mut erased_value_executor);
            let erased_unsafe_executor = UnsafeErasedLocalExecutor::new(erased_value_executor_ref);
            TASK_LOCAL_EXECUTOR.with(|e| {
                e.borrow_mut().replace(Box::new(erased_unsafe_executor));
            });
        }
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
            let read_executor = e.take().expect("Executor not set");
            *executor = read_executor
        });
        TASK_LOCAL_EXECUTOR.with_borrow_mut(|e| {
            *e = None;
        });
    }
    match r {
        Poll::Ready(r) => {
            sender.send(r);
            Poll::Ready(())
        }
        Poll::Pending => Poll::Pending,
    }
}

impl<F: Future, ONotifier, E> SpawnedTask<F, ONotifier, E>
where
    ONotifier: ObserverNotified<F::Output>,
{
    /**
    Polls the task.  This has the standard semantics for Rust futures.

    # Parameters
    - `cx`: The context for the poll.
    - `local_executor`: A local executor, if available.  This will be used to populate the thread-local [TASK_LOCAL_EXECUTOR] variable.
    */
    fn poll<'l, L>(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        local_executor: Option<&mut L>,
    ) -> std::task::Poll<()>
    where
        L: SomeLocalExecutor<'l>,
    {
        //destructure
        let poll_after = self.poll_after();
        let (future, sender, label, priority, cancellation, task_id, executor) = unsafe {
            let unchecked = self.get_unchecked_mut();
            let future = Pin::new_unchecked(&mut unchecked.task);
            let sender = Pin::new_unchecked(&mut unchecked.sender);
            let label = Pin::new_unchecked(&mut unchecked.label);
            let priority = Pin::new_unchecked(&mut unchecked.priority);
            let cancellation = Pin::new_unchecked(&mut unchecked.cancellation);
            let task_id = unchecked.task_id;
            let executor = Pin::new_unchecked(&mut unchecked.executor);
            (
                future,
                sender,
                label,
                priority,
                cancellation,
                task_id,
                executor,
            )
        };

        common_poll(
            future,
            sender.get_mut(),
            label.get_mut(),
            cancellation.get_mut(),
            executor.get_mut(),
            local_executor,
            *priority.get_mut(),
            task_id,
            poll_after,
            cx,
        )
    }
}

impl<F, ONotifier, E> Future for SpawnedTask<F, ONotifier, E>
where
    F: Future,
    ONotifier: ObserverNotified<F::Output>,
{
    type Output = ();
    /**
        Implements Future trait by declining to set a local context.
    */
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        SpawnedTask::poll::<Infallible>(self, cx, None)
    }
}

impl<'executor, F, ONotifier, Executor: SomeLocalExecutor<'executor>>
    SpawnedLocalTask<F, ONotifier, Executor>
where
    F: Future,
    ONotifier: ObserverNotified<F::Output>,
{
    //I can't believe it's not future

    /**
    Polls the task.  This has the standard semantics for Rust futures.

    # Parameters
    - `cx`: The context for the poll.
    - `executor`: The executor the task was spawned on.  This will be used to populate the thread-local [TASK_LOCAL_EXECUTOR] variable.
    - `some_executor`: An executor for spawning new tasks, if desired.  This is used to populate the task-local [TASK_EXECUTOR] variable.
    */
    pub fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        executor: &mut Executor,
        mut some_executor: Option<Box<(dyn SomeExecutor<ExecutorNotifier = Infallible> + 'static)>>,
    ) -> std::task::Poll<()> {
        let poll_after = self.poll_after();
        //destructure
        let (future, sender, label, priority, cancellation, task_id) = unsafe {
            let unchecked = self.get_unchecked_mut();
            let future = Pin::new_unchecked(&mut unchecked.task);
            let sender = Pin::new_unchecked(&mut unchecked.sender);
            let label = Pin::new_unchecked(&mut unchecked.label);
            let priority = Pin::new_unchecked(&mut unchecked.priority);
            let cancellation = Pin::new_unchecked(&mut unchecked.cancellation);
            let task_id = unchecked.task_id;
            (future, sender, label, priority, cancellation, task_id)
        };

        common_poll(
            future,
            sender.get_mut(),
            label.get_mut(),
            cancellation.get_mut(),
            &mut some_executor,
            Some(executor),
            *priority.get_mut(),
            task_id,
            poll_after,
            cx,
        )
    }
}

impl
    Task<
        Pin<Box<dyn Future<Output = Box<dyn Any + Send + 'static>> + Send + 'static>>,
        Box<dyn ObserverNotified<dyn Any + Send> + Send>,
    >
{
    /**
            Creates a new objsafe future
    */
    pub fn new_objsafe(
        label: String,
        future: Box<dyn Future<Output = Box<dyn Any + Send + 'static>> + Send + 'static>,
        configuration: Configuration,
        notifier: Option<Box<dyn ObserverNotified<dyn Any + Send> + Send>>,
    ) -> Self {
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
    poll_after: crate::sys::Instant,
}

/**
A builder for [Configuration].
*/
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ConfigurationBuilder {
    hint: Option<Hint>,
    priority: Option<Priority>,
    poll_after: Option<crate::sys::Instant>,
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
    pub fn poll_after(mut self, poll_after: crate::sys::Instant) -> Self {
        self.poll_after = Some(poll_after);
        self
    }

    pub fn build(self) -> Configuration {
        Configuration {
            hint: self.hint.unwrap_or_else(|| Hint::default()),
            priority: self.priority.unwrap_or_else(|| priority::Priority::Unknown),
            poll_after: self
                .poll_after
                .unwrap_or_else(|| crate::sys::Instant::now()),
        }
    }
}

impl Configuration {
    pub fn new(hint: Hint, priority: priority::Priority, poll_after: crate::sys::Instant) -> Self {
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
pub trait DynLocalSpawnedTask<Executor> {
    fn poll<'executor>(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        executor: &'executor mut Executor,
        some_executor: Option<Box<(dyn SomeExecutor<ExecutorNotifier = Infallible> + 'static)>>,
    ) -> std::task::Poll<()>;
    fn poll_after(&self) -> crate::sys::Instant;
    fn label(&self) -> &str;

    fn task_id(&self) -> TaskID;

    fn hint(&self) -> Hint;
    fn priority(&self) -> priority::Priority;
}

impl<'executor, F, ONotifier, Executor> DynLocalSpawnedTask<Executor>
    for SpawnedLocalTask<F, ONotifier, Executor>
where
    F: Future,
    Executor: SomeLocalExecutor<'executor>,
    ONotifier: ObserverNotified<F::Output>,
{
    fn poll<'ex>(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        executor: &'ex mut Executor,
        some_executor: Option<Box<(dyn SomeExecutor<ExecutorNotifier = Infallible> + 'static)>>,
    ) -> std::task::Poll<()> {
        SpawnedLocalTask::poll(self, cx, executor, some_executor)
    }

    fn poll_after(&self) -> crate::sys::Instant {
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

    fn task_id(&self) -> TaskID {
        self.task_id()
    }
}

/**
ObjSafe type-erased wrapper for [SpawnedTask].

If you have no relevant type parameter, choose [Infallible].
*/
pub trait DynSpawnedTask<LocalExecutorType>: Send + Debug {
    fn poll<'l>(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        local_executor: Option<&mut LocalExecutorType>,
    ) -> std::task::Poll<()>
    where
        LocalExecutorType: SomeLocalExecutor<'l>;

    fn poll_after(&self) -> crate::sys::Instant;
    fn label(&self) -> &str;

    fn task_id(&self) -> TaskID;

    fn hint(&self) -> Hint;
    fn priority(&self) -> priority::Priority;
}

impl<F: Future, N: ObserverNotified<<F as Future>::Output>, E, L> DynSpawnedTask<L>
    for SpawnedTask<F, N, E>
where
    F: Send,
    E: Send,
    N: Send,
    F::Output: Send,
{
    fn poll<'l>(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        local_executor: Option<&mut L>,
    ) -> Poll<()>
    where
        L: SomeLocalExecutor<'l>,
    {
        SpawnedTask::poll(self, cx, local_executor)
    }

    fn poll_after(&self) -> crate::sys::Instant {
        self.poll_after()
    }

    fn label(&self) -> &str {
        self.label()
    }

    fn task_id(&self) -> TaskID {
        self.task_id()
    }

    fn hint(&self) -> Hint {
        self.hint()
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
            poll_after: crate::sys::Instant::now(),
        }
    }
}

/*
I don't think it makes sense to support Clone on Task.
That eliminates the need for PartialEq, Eq, Hash.  We have ID type for this.

I suppose we could implement Default with a blank task...

 */

/**
A future that always returns Ready(()).
*/
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
pub struct DefaultFuture;
impl Future for DefaultFuture {
    type Output = ();
    fn poll(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Poll::Ready(())
    }
}
impl Default for Task<DefaultFuture, Infallible> {
    fn default() -> Self {
        Task::with_notifications(
            "".to_string(),
            DefaultFuture,
            Configuration::default(),
            None,
        )
    }
}

impl<F: Future, N, E> Debug for SpawnedTask<F, N, E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpawnedTask")
            .field("poll_after", &self.poll_after)
            .field("hint", &self.hint)
            .field("label", &self.label)
            .field("priority", &self.priority)
            .field("task_id", &self.task_id)
            .finish()
    }
}

impl<F: Future, N, E> Debug for SpawnedLocalTask<F, N, E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpawnedLocalTask")
            .field("poll_after", &self.poll_after)
            .field("hint", &self.hint)
            .field("label", &self.label)
            .field("priority", &self.priority)
            .field("task_id", &self.task_id)
            .finish()
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

impl<F: Future, N, E> AsRef<F> for SpawnedTask<F, N, E> {
    fn as_ref(&self) -> &F {
        &self.task
    }
}

impl<F: Future, N, E> AsMut<F> for SpawnedTask<F, N, E> {
    fn as_mut(&mut self) -> &mut F {
        &mut self.task
    }
}

impl<F: Future, N, E> AsRef<F> for SpawnedLocalTask<F, N, E> {
    fn as_ref(&self) -> &F {
        &self.task
    }
}

impl<F: Future, N, E> AsMut<F> for SpawnedLocalTask<F, N, E> {
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

//taskID.  I think we want to support various conversions to and from u64

impl From<u64> for TaskID {
    /**
    Equivalent to [TaskID::from_u64].
    */
    fn from(id: u64) -> Self {
        TaskID::from_u64(id)
    }
}

impl From<TaskID> for u64 {
    /**
    Equivalent to [TaskID::to_u64].
    */

    fn from(id: TaskID) -> u64 {
        id.to_u64()
    }
}

impl AsRef<u64> for TaskID {
    /**
    Equivalent to [TaskID::to_u64].
    */
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

/*dyn traits boilerplate

Don't want to implement eq, etc. at this time –use task ID.

AsRef / sure, why not
 */

impl<'a, F, N, E> AsRef<dyn DynSpawnedTask<Infallible> + 'a> for SpawnedTask<F, N, E>
where
    N: ObserverNotified<F::Output>,
    F: Future + 'a,
    F: Send,
    N: Send,
    E: Send + 'a,
    F::Output: Send,
{
    fn as_ref(&self) -> &(dyn DynSpawnedTask<Infallible> + 'a) {
        self
    }
}

impl<'a, F, N, E> AsMut<dyn DynSpawnedTask<Infallible> + 'a> for SpawnedTask<F, N, E>
where
    N: ObserverNotified<F::Output>,
    F: Future + 'a,
    F: Send,
    N: Send,
    E: Send + 'a,
    F::Output: Send,
{
    fn as_mut(&mut self) -> &mut (dyn DynSpawnedTask<Infallible> + 'a) {
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::observer::{FinishedObservation, Observer, ObserverNotified};
    use crate::task::{DynLocalSpawnedTask, DynSpawnedTask, SpawnedTask, Task};
    use crate::{task_local, SomeExecutor, SomeLocalExecutor};
    use std::any::Any;
    use std::convert::Infallible;
    use std::future::Future;
    use std::pin::Pin;

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_create_task() {
        let task: Task<_, Infallible> =
            Task::with_notifications("test".to_string(), async {}, Default::default(), None);
        assert_eq!(task.label(), "test");
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_create_no_notify() {
        let t = Task::without_notifications("test".to_string(), async {}, Default::default());
        assert_eq!(t.label(), "test");
    }
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_send() {
        task_local!(
            static FOO: u32;
        );

        let scoped = FOO.scope(42, async {});

        fn assert_send<T: Send>(_: T) {}
        assert_send(scoped);
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_dyntask_objsafe() {
        let _d: &dyn DynSpawnedTask<Infallible>;
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
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
        fn spawn_check<F: Future + Send, E: SomeExecutor>(task: Task<F, Infallible>, exec: &mut E)
        where
            F::Output: Send,
            E: Send,
        {
            let spawned: SpawnedTask<F, Infallible, E> = task.spawn(exec).0;
            fn assert_send<T: Send>(_: T) {}
            assert_send(spawned);
        }

        #[allow(unused)]
        fn spawn_check_sync<F: Future + Sync, E: SomeExecutor>(
            task: Task<F, Infallible>,
            exec: &mut E,
        ) where
            F::Output: Send,
            E::ExecutorNotifier: Sync,
        {
            let spawned: SpawnedTask<F, Infallible, E> = task.spawn(exec).0;
            fn assert_sync<T: Sync>(_: T) {}
            assert_sync(spawned);
        }

        #[allow(unused)]
        fn spawn_check_unpin<F: Future + Unpin, E: SomeExecutor>(
            task: Task<F, Infallible>,
            exec: &mut E,
        ) where
            E: Unpin,
        {
            let spawned: SpawnedTask<F, Infallible, E> = task.spawn(exec).0;
            fn assert_unpin<T: Unpin>(_: T) {}
            assert_unpin(spawned);
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_local_executor() {
        struct ExLocalExecutor<'future>(
            Vec<Pin<Box<dyn DynLocalSpawnedTask<ExLocalExecutor<'future>> + 'future>>>,
        );

        impl<'existing_tasks, 'new_task> SomeLocalExecutor<'new_task> for ExLocalExecutor<'existing_tasks>
        where
            'new_task: 'existing_tasks,
        {
            type ExecutorNotifier = Infallible;

            fn spawn_local<F: Future, Notifier: ObserverNotified<F::Output>>(
                &mut self,
                task: Task<F, Notifier>,
            ) -> impl Observer<Value = F::Output>
            where
                Self: Sized,
                F: 'new_task,
                F::Output: 'static,
                /* I am a little uncertain whether this is really required */
                <F as Future>::Output: Unpin,
            {
                let (spawn, observer) = task.spawn_local(self);
                let pinned_spawn = Box::pin(spawn);
                self.0.push(pinned_spawn);
                observer
            }

            fn spawn_local_async<F: Future, Notifier: ObserverNotified<F::Output>>(
                &mut self,
                task: Task<F, Notifier>,
            ) -> impl Future<Output = impl Observer<Value = F::Output>>
            where
                Self: Sized,
                F: 'new_task,
                F::Output: 'static,
            {
                async {
                    let (spawn, observer) = task.spawn_local(self);
                    let pinned_spawn = Box::pin(spawn);
                    self.0.push(pinned_spawn);
                    observer
                }
            }

            fn spawn_local_objsafe(
                &mut self,
                task: Task<
                    Pin<Box<dyn Future<Output = Box<dyn Any>>>>,
                    Box<dyn ObserverNotified<(dyn Any + 'static)>>,
                >,
            ) -> Box<dyn Observer<Value = Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>>
            {
                let (spawn, observer) = task.spawn_local_objsafe(self);
                let pinned_spawn = Box::pin(spawn);
                self.0.push(pinned_spawn);
                Box::new(observer)
            }

            fn spawn_local_objsafe_async<'s>(
                &'s mut self,
                task: Task<
                    Pin<Box<dyn Future<Output = Box<dyn Any>>>>,
                    Box<dyn ObserverNotified<(dyn Any + 'static)>>,
                >,
            ) -> Box<
                dyn Future<
                        Output = Box<
                            dyn Observer<
                                Value = Box<dyn Any>,
                                Output = FinishedObservation<Box<dyn Any>>,
                            >,
                        >,
                    > + 's,
            > {
                Box::new(async {
                    let (spawn, observer) = task.spawn_local_objsafe(self);
                    let pinned_spawn = Box::pin(spawn);
                    self.0.push(pinned_spawn);
                    Box::new(observer)
                        as Box<
                            dyn Observer<
                                Value = Box<dyn Any>,
                                Output = FinishedObservation<Box<dyn Any>>,
                            >,
                        >
                })
            }

            fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
                todo!()
            }
        }
    }
}
