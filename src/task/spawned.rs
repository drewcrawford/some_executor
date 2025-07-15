// SPDX-License-Identifier: MIT OR Apache-2.0

//! Spawned task types and implementations for the some_executor framework.
//!
//! This module contains the spawned task types that represent tasks after they have been
//! submitted to an executor for execution. These include:
//!
//! - [`SpawnedTask`]: A task spawned on a Send executor
//! - [`SpawnedLocalTask`]: A task spawned on a local (!Send) executor  
//! - [`SpawnedStaticTask`]: A task spawned on a static executor
//!
//! The module also contains the common polling infrastructure used by all spawned task types.

use super::{TaskID, task_local::InFlightTaskCancellation};
use crate::hint::Hint;
use crate::observer::{ObserverNotified, ObserverSender};
use crate::{Priority, SomeExecutor, SomeLocalExecutor, SomeStaticExecutor};
use std::convert::Infallible;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

// Re-export task-local variables that are used in common_poll
use super::task_local::{
    IS_CANCELLED, TASK_EXECUTOR, TASK_ID, TASK_LABEL, TASK_LOCAL_EXECUTOR, TASK_PRIORITY,
    TASK_STATIC_EXECUTOR,
};
use crate::local::UnsafeErasedLocalExecutor;

/// Task metadata and state to reduce parameter count in common_poll function
pub(super) struct TaskMetadata {
    pub(super) priority: Priority,
    pub(super) task_id: TaskID,
    pub(super) poll_after: crate::sys::Instant,
}

/// Grouped mutable references to reduce parameter count in common_poll function
pub(super) struct TaskState<'a, F, N> {
    pub(super) sender: &'a mut ObserverSender<F, N>,
    pub(super) label: &'a mut Option<String>,
    pub(super) cancellation: &'a mut Option<InFlightTaskCancellation>,
    pub(super) executor: &'a mut Option<Box<dyn SomeExecutor<ExecutorNotifier = Infallible>>>,
}

/// A task that has been spawned onto an executor and is ready to be polled.
///
/// `SpawnedTask` represents a task that has been submitted to an executor. It contains
/// the original future along with all the metadata and infrastructure needed for the
/// executor to poll it to completion.
///
/// This type implements `Future`, allowing executors to poll it directly. During polling,
/// it manages task-local variables and sends the result to any attached observers upon
/// completion.
///
/// # Type Parameters
///
/// * `F` - The underlying future type
/// * `ONotifier` - The observer notification type for task completion
/// * `Executor` - The executor type that spawned this task
///
/// # Task-Local Context
///
/// When polled, this task automatically sets up task-local variables that the future
/// can access:
/// - [`TASK_LABEL`] - The task's label
/// - [`TASK_ID`] - The task's unique identifier
/// - [`TASK_PRIORITY`] - The task's priority
/// - [`IS_CANCELLED`] - Cancellation status
/// - [`TASK_EXECUTOR`] - Reference to the executor
pub struct SpawnedTask<F, ONotifier, Executor>
where
    F: Future,
{
    pub(super) task: F,
    pub(super) sender: ObserverSender<F::Output, ONotifier>,
    pub(super) phantom: PhantomData<Executor>,
    pub(super) poll_after: crate::sys::Instant,
    pub(super) hint: Hint,
    //these task_local properties optional so we can take/replace them
    pub(super) label: Option<String>,
    pub(super) cancellation: Option<InFlightTaskCancellation>,
    pub(super) executor: Option<Box<dyn SomeExecutor<ExecutorNotifier = Infallible>>>,
    pub(super) priority: Priority, //copy,so no need to repair/replace them, boring
    pub(super) task_id: TaskID,    //boring
}

/// A task that has been spawned onto a local executor.
///
/// `SpawnedLocalTask` is similar to [`SpawnedTask`] but designed for executors that
/// are not `Send` and are tied to a specific thread. Unlike `SpawnedTask`, this type
/// does not implement `Future` directly because local executors need to be injected
/// during each poll operation.
///
/// # Type Parameters
///
/// * `F` - The underlying future type
/// * `ONotifier` - The observer notification type for task completion
/// * `Executor` - The local executor type that spawned this task
///
/// # Design Note
///
/// Unlike [`SpawnedTask`] which embeds a copy of the executor, `SpawnedLocalTask` only
/// stores a `PhantomData` marker. This is because local executors may be implemented
/// as references that cannot be stored. Instead, the executor must be provided explicitly
/// when polling the task.
///
/// # Task-Local Context
///
/// When polled via [`poll`](SpawnedLocalTask::poll), this task sets up the same task-local
/// variables as [`SpawnedTask`], with the addition of:
/// - [`TASK_LOCAL_EXECUTOR`] - Reference to the local executor
///
/// # Examples
///
/// Local tasks must be polled explicitly with a reference to their executor:
///
/// ```no_run
/// # use std::convert::Infallible;
/// use some_executor::task::SpawnedLocalTask;
/// # use std::task::Context;
/// # type MyLocalExecutor = Infallible;
/// # type MyFuture = std::future::Ready<()>;
/// # type MyNotifier = std::convert::Infallible;
/// # let mut spawned: SpawnedLocalTask<MyFuture, MyNotifier, MyLocalExecutor> = todo!();
/// # let mut executor: MyLocalExecutor = todo!();
/// # let mut context: Context = todo!();
/// use std::pin::Pin;
///
/// // Poll the local task with its executor
/// let poll_result = Pin::new(&mut spawned).poll(&mut context, &mut executor, None);
/// ```
pub struct SpawnedLocalTask<F, ONotifier, Executor>
where
    F: Future,
{
    pub(super) task: F,
    pub(super) sender: ObserverSender<F::Output, ONotifier>,
    pub(super) poll_after: crate::sys::Instant,
    pub(super) executor: PhantomData<Executor>,
    //these task-local properties are optional so we can move them in and out
    pub(super) label: Option<String>,
    pub(super) cancellation: Option<InFlightTaskCancellation>,
    //copy-safe task-local properties
    pub(super) hint: Hint,
    pub(super) priority: Priority,
    pub(super) task_id: TaskID,
}

/// A task that has been spawned onto a static executor.
///
/// `SpawnedStaticTask` is designed for executors that handle static futures without
/// requiring `Send`. This is useful for thread-local executors that work with static
/// data but don't need to cross thread boundaries.
///
/// # Type Parameters
///
/// * `F` - The underlying future type (must be `'static`)
/// * `ONotifier` - The observer notification type for task completion
/// * `Executor` - The static executor type that spawned this task
///
/// # Design Note
///
/// Unlike [`SpawnedTask`] which requires `Send`, `SpawnedStaticTask` only requires
/// `'static`. This allows for static data access without the overhead of Send
/// synchronization.
///
/// # Task-Local Context
///
/// When polled, this task sets up the same task-local variables as other spawned
/// tasks, but without the `Send` requirement.
pub struct SpawnedStaticTask<F, ONotifier, Executor>
where
    F: Future,
{
    pub(super) task: F,
    pub(super) sender: ObserverSender<F::Output, ONotifier>,
    pub(super) poll_after: crate::sys::Instant,
    pub(super) executor: PhantomData<Executor>,
    //these task-local properties are optional so we can move them in and out
    pub(super) label: Option<String>,
    pub(super) cancellation: Option<InFlightTaskCancellation>,
    //copy-safe task-local properties
    pub(super) hint: Hint,
    pub(super) priority: Priority,
    pub(super) task_id: TaskID,
}

impl<F: Future, ONotifier, ENotifier> SpawnedTask<F, ONotifier, ENotifier> {
    /// Returns the execution hint for this spawned task.
    pub fn hint(&self) -> Hint {
        self.hint
    }

    /// Returns the label of this spawned task.
    ///
    /// # Panics
    ///
    /// Panics if called while the task is being polled, as the label is temporarily
    /// moved into task-local storage during polling.
    pub fn label(&self) -> &str {
        self.label.as_ref().expect("Future is polling")
    }

    /// Returns the priority of this spawned task.
    pub fn priority(&self) -> Priority {
        self.priority
    }

    /// Returns the earliest time this task should be polled.
    ///
    /// Executors should respect this time and not poll the task before it.
    pub fn poll_after(&self) -> crate::sys::Instant {
        self.poll_after
    }

    /// Returns the unique identifier for this spawned task.
    pub fn task_id(&self) -> TaskID {
        self.task_id
    }

    /// Consumes the spawned task and returns the underlying future.
    ///
    /// This is useful if you need to extract the future from the spawned task wrapper,
    /// but note that you'll lose the task metadata and observer infrastructure.
    pub fn into_future(self) -> F {
        self.task
    }
}

impl<F: Future, ONotifier, Executor> SpawnedLocalTask<F, ONotifier, Executor> {
    /// Returns the execution hint for this spawned local task.
    pub fn hint(&self) -> Hint {
        self.hint
    }

    /// Returns the label of this spawned local task.
    ///
    /// # Panics
    ///
    /// Panics if called while the task is being polled, as the label is temporarily
    /// moved into task-local storage during polling.
    pub fn label(&self) -> &str {
        self.label.as_ref().expect("Future is polling")
    }

    /// Returns the priority of this spawned local task.
    pub fn priority(&self) -> Priority {
        self.priority
    }

    /// Returns the earliest time this task should be polled.
    ///
    /// Executors should respect this time and not poll the task before it.
    pub fn poll_after(&self) -> crate::sys::Instant {
        self.poll_after
    }

    /// Returns the unique identifier for this spawned local task.
    pub fn task_id(&self) -> TaskID {
        self.task_id
    }

    /// Consumes the spawned local task and returns the underlying future.
    ///
    /// This is useful if you need to extract the future from the spawned task wrapper,
    /// but note that you'll lose the task metadata and observer infrastructure.
    pub fn into_future(self) -> F {
        self.task
    }
}

impl<F: Future, ONotifier, Executor> SpawnedStaticTask<F, ONotifier, Executor> {
    /// Returns the execution hint for this spawned static task.
    pub fn hint(&self) -> Hint {
        self.hint
    }

    /// Returns the label of this spawned static task.
    ///
    /// # Panics
    ///
    /// Panics if called while the task is being polled, as the label is temporarily
    /// moved into task-local storage during polling.
    pub fn label(&self) -> &str {
        self.label.as_ref().expect("Future is polling")
    }

    /// Returns the priority of this spawned static task.
    pub fn priority(&self) -> Priority {
        self.priority
    }

    /// Returns the earliest time this task should be polled.
    ///
    /// Executors should respect this time and not poll the task before it.
    pub fn poll_after(&self) -> crate::sys::Instant {
        self.poll_after
    }

    /// Returns the unique identifier for this spawned static task.
    pub fn task_id(&self) -> TaskID {
        self.task_id
    }

    /// Consumes the spawned static task and returns the underlying future.
    ///
    /// This is useful if you need to extract the future from the spawned task wrapper,
    /// but note that you'll lose the task metadata and observer infrastructure.
    pub fn into_future(self) -> F {
        self.task
    }

    /// Polls the spawned static task.
    ///
    /// This polls the underlying future while setting up the appropriate task-local
    /// context, similar to other spawned task types but without the `Send` requirement.
    ///
    /// # Parameters
    /// - `cx`: The context for the poll.
    /// - `static_executor`: An optional static executor for spawning new !Send tasks,
    ///   used to populate the task-local [TASK_STATIC_EXECUTOR] variable.
    /// - `some_executor`: An optional executor for spawning new tasks, used to populate
    ///   the task-local [TASK_EXECUTOR] variable.
    ///
    /// # Returns
    /// `Poll::Ready(())` when the task completes or is cancelled, `Poll::Pending` otherwise.
    pub(crate) fn poll<S>(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        static_executor: Option<&mut S>,
        mut some_executor: Option<Box<dyn SomeExecutor<ExecutorNotifier = Infallible> + 'static>>,
    ) -> std::task::Poll<()>
    where
        ONotifier: ObserverNotified<F::Output>,
        S: SomeStaticExecutor,
    {
        let poll_after = self.poll_after();
        //destructure
        let (future, sender, label, priority, cancellation, task_id) = unsafe {
            let unchecked = self.get_unchecked_mut();
            let future = Pin::new_unchecked(&mut unchecked.task);
            let sender = &mut unchecked.sender;
            let label = &mut unchecked.label;
            let priority = unchecked.priority;
            let cancellation = &mut unchecked.cancellation;
            let task_id = unchecked.task_id;
            (future, sender, label, priority, cancellation, task_id)
        };

        let metadata = TaskMetadata {
            priority,
            task_id,
            poll_after,
        };
        let state = TaskState {
            sender,
            label,
            cancellation,
            executor: &mut some_executor,
        };
        // Note: We pass None for local_executor since SpawnedStaticTask doesn't have a local executor parameter
        common_poll(
            future,
            state,
            None::<&mut Infallible>,
            static_executor,
            metadata,
            cx,
        )
    }
}

pub(super) fn common_poll<'l, F, N, L, S>(
    future: Pin<&mut F>,
    state: TaskState<'_, F::Output, N>,
    local_executor: Option<&mut L>,
    static_executor: Option<&mut S>,
    metadata: TaskMetadata,
    cx: &mut Context,
) -> std::task::Poll<()>
where
    F: Future,
    N: ObserverNotified<F::Output>,
    L: SomeLocalExecutor<'l>,
    S: SomeStaticExecutor,
{
    assert!(
        metadata.poll_after <= crate::sys::Instant::now(),
        "Conforming executors should not poll tasks before the poll_after time."
    );
    if state.sender.observer_cancelled() {
        //we don't really need to notify the observer here.  Also the notifier will run upon drop.
        return Poll::Ready(());
    }
    //before poll, we need to set our properties
    unsafe {
        TASK_LABEL.with_mut(|l| {
            *l = Some(
                state
                    .label
                    .take()
                    .expect("Label not set (is task being polled already?)"),
            );
        });
        IS_CANCELLED.with_mut(|c| {
            *c = Some(
                state
                    .cancellation
                    .take()
                    .expect("Cancellation not set (is task being polled already?)"),
            );
        });
        TASK_PRIORITY.with_mut(|p| {
            *p = Some(metadata.priority);
        });
        TASK_ID.with_mut(|i| {
            *i = Some(metadata.task_id);
        });
        TASK_EXECUTOR.with_mut(|e| {
            *e = Some(state.executor.take());
        });
        if let Some(_static_executor) = static_executor {
            // For now, we'll just handle static executors by setting the task-local
            // TODO: Add proper type erasure for static executors similar to local executors
            // TASK_STATIC_EXECUTOR.with_mut(|e| {
            //     *e = Some(Box::new(StaticExecutorWrapper::new(_static_executor)));
            // });
        }
        if let Some(local_executor) = local_executor {
            let mut erased_value_executor = Box::new(
                crate::local::SomeLocalExecutorErasingNotifier::new(local_executor),
            )
                as Box<
                    dyn SomeLocalExecutor<
                            ExecutorNotifier = Box<dyn crate::observer::ExecutorNotified>,
                        > + '_,
                >;
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
            *state.label = Some(read_label);
        });
        IS_CANCELLED.with_mut(|c| {
            let read_cancellation = c.take().expect("Cancellation not set");
            *state.cancellation = Some(read_cancellation);
        });
        TASK_PRIORITY.with_mut(|p| {
            *p = None;
        });
        TASK_ID.with_mut(|i| {
            *i = None;
        });
        TASK_EXECUTOR.with_mut(|e| {
            let read_executor = e.take().expect("Executor not set");
            *state.executor = read_executor
        });
        TASK_STATIC_EXECUTOR.with_mut(|e| {
            *e = None;
        });
        TASK_LOCAL_EXECUTOR.with_borrow_mut(|e| {
            *e = None;
        });
    }
    match r {
        Poll::Ready(r) => {
            state.sender.send(r);
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
    pub fn poll<'l, L>(
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

        let metadata = TaskMetadata {
            priority: *priority.get_mut(),
            task_id,
            poll_after,
        };
        let state = TaskState {
            sender: sender.get_mut(),
            label: label.get_mut(),
            cancellation: cancellation.get_mut(),
            executor: executor.get_mut(),
        };
        common_poll(
            future,
            state,
            local_executor,
            None::<&mut Infallible>,
            metadata,
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
        mut some_executor: Option<Box<dyn SomeExecutor<ExecutorNotifier = Infallible> + 'static>>,
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

        let metadata = TaskMetadata {
            priority: *priority.get_mut(),
            task_id,
            poll_after,
        };
        let state = TaskState {
            sender: sender.get_mut(),
            label: label.get_mut(),
            cancellation: cancellation.get_mut(),
            executor: &mut some_executor,
        };
        common_poll(
            future,
            state,
            Some(executor),
            None::<&mut Infallible>,
            metadata,
            cx,
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

impl<F: Future, N, E> Debug for SpawnedStaticTask<F, N, E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpawnedStaticTask")
            .field("poll_after", &self.poll_after)
            .field("hint", &self.hint)
            .field("label", &self.label)
            .field("priority", &self.priority)
            .field("task_id", &self.task_id)
            .finish()
    }
}

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

impl<F: Future, N, E> AsRef<F> for SpawnedStaticTask<F, N, E> {
    fn as_ref(&self) -> &F {
        &self.task
    }
}

impl<F: Future, N, E> AsMut<F> for SpawnedStaticTask<F, N, E> {
    fn as_mut(&mut self) -> &mut F {
        &mut self.task
    }
}

// Dynamic trait implementations for SpawnedTask
use crate::task::dyn_spawned::DynSpawnedTask;

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
