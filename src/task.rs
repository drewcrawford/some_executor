//SPDX-License-Identifier: MIT OR Apache-2.0

//! Task management and execution primitives for the some_executor framework.
//!
//! This module provides the core types and traits for creating, configuring, and managing
//! asynchronous tasks within the executor ecosystem. Tasks are the fundamental unit of
//! asynchronous work that executors poll to completion.
//!
//! # Overview
//!
//! The task system is built around several key concepts:
//!
//! - **[`Task`]**: A top-level future with metadata like labels, priorities, and hints
//! - **[`SpawnedTask`]/[`SpawnedLocalTask`]**: Tasks that have been spawned onto an executor
//! - **[`TaskID`]**: Unique identifiers for tracking tasks across the system
//! - **[`Configuration`]**: Runtime hints and scheduling preferences for tasks
//!
//! # Task Lifecycle
//!
//! 1. **Creation**: Tasks are created with a future, label, and configuration
//! 2. **Spawning**: Tasks are spawned onto an executor, producing a spawned task and observer
//! 3. **Polling**: The executor polls the spawned task until completion
//! 4. **Completion**: The task's result is sent to any attached observers
//!
//! # Examples
//!
//! ## Basic Task Creation and Spawning
//!
//! ```
//! use some_executor::task::{Task, Configuration};
//! use some_executor::current_executor::current_executor;
//! use some_executor::SomeExecutor;
//! use some_executor::observer::Observer;
//!
//! # async fn example() {
//! let task = Task::without_notifications(
//!     "my-task".to_string(),
//!     Configuration::default(),
//!     async {
//!         println!("Hello from task!");
//!         42
//!     },
//! );
//!
//! let mut executor = current_executor();
//! let observer = executor.spawn(task).detach();
//!
//! // Task runs asynchronously
//! # }
//! ```
//!
//! ## Task Configuration
//!
//! ```
//! use some_executor::task::{Task, ConfigurationBuilder};
//! use some_executor::hint::Hint;
//! use some_executor::Priority;
//!
//! # async fn example() {
//! let config = ConfigurationBuilder::new()
//!     .hint(Hint::CPU)
//!     .priority(Priority::unit_test())
//!     .build();
//!
//! let task = Task::without_notifications(
//!     "high-priority-cpu".to_string(),
//!     config,
//!     async {
//!         // Some CPU-intensive operation
//!         let mut sum = 0u64;
//!         for i in 0..1000000 {
//!             sum += i;
//!         }
//!     },
//! );
//! # }
//! ```
//!
//! ## Task-Local Variables
//!
//! Tasks have access to task-local variables that provide context during execution.
//! * [`TASK_LABEL`] - The task's label
//! * [`TASK_ID`] - The task's unique identifier
//! * [`IS_CANCELLED`] - Cancellation status
//!

use crate::dyn_observer_notified::ObserverNotifiedErased;
use crate::hint::Hint;
use crate::local::UnsafeErasedLocalExecutor;
use crate::observer::{
    ExecutorNotified, ObserverNotified, ObserverSender, TypedObserver, observer_channel,
};
use crate::{DynExecutor, Priority, SomeExecutor, SomeLocalExecutor, task_local};
use std::any::Any;
use std::cell::RefCell;
use std::convert::Infallible;
use std::fmt::{Debug, Formatter};
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::task::{Context, Poll};

/// A unique identifier for a task.
///
/// `TaskID` provides a way to uniquely identify and track tasks throughout their lifecycle.
/// Each task is assigned a unique ID when created, which remains constant even as the task
/// moves between executors or changes state.
///
/// Task IDs are globally unique within a process and are generated using an atomic counter,
/// ensuring thread-safe ID allocation.
///
/// # Examples
///
/// ```
/// use some_executor::task::{Task, TaskID, Configuration};
///
/// let task = Task::without_notifications(
///     "example".to_string(),
///     Configuration::default(),
///     async { 42 },
/// );
///
/// let id = task.task_id();
///
/// // Task IDs can be converted to/from u64 for serialization
/// let id_value = id.to_u64();
/// let restored_id = TaskID::from_u64(id_value);
/// assert_eq!(id, restored_id);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskID(u64);

impl TaskID {
    /// Creates a task ID from a u64 value.
    ///
    /// This method is primarily intended for deserialization or restoring task IDs
    /// that were previously obtained via [`to_u64`](Self::to_u64).
    ///
    /// # Arguments
    ///
    /// * `id` - A u64 value representing a task ID
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::TaskID;
    ///
    /// let id = TaskID::from_u64(12345);
    /// assert_eq!(id.to_u64(), 12345);
    /// ```
    pub fn from_u64(id: u64) -> Self {
        TaskID(id)
    }

    /// Converts the task ID to a u64 value.
    ///
    /// This method is useful for serialization, logging, or any scenario where
    /// you need to represent the task ID as a primitive value.
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::TaskID;
    ///
    /// let id = TaskID::from_u64(42);
    /// let value = id.to_u64();
    /// assert_eq!(value, 42);
    /// ```
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

/// A top-level future with associated metadata for execution.
///
/// `Task` wraps a future with additional information that helps executors make
/// scheduling decisions and provides context during execution. Unlike raw futures,
/// tasks carry metadata such as:
///
/// - A human-readable label for debugging and monitoring
/// - Execution hints (e.g., whether the task will block)
/// - Priority information for scheduling
/// - Optional notification callbacks for completion
/// - A unique task ID for tracking
///
/// # Type Parameters
///
/// * `F` - The underlying future type
/// * `N` - The notification type (use `Infallible` if no notifications are needed)
///
/// # Examples
///
/// ## Creating a simple task
///
/// ```
/// use some_executor::task::{Task, Configuration};
/// use std::convert::Infallible;
///
/// let task: Task<_, Infallible> = Task::without_notifications(
///     "fetch-data".to_string(),
///     Configuration::default(),
///     async {
///         // Simulate fetching data
///         "data from server"
///     },
/// );
/// ```
///
/// ## Creating a task with notifications
///
/// ```
/// use some_executor::task::{Task, Configuration};
/// use some_executor::observer::ObserverNotified;
///
/// # struct MyNotifier;
/// # impl ObserverNotified<String> for MyNotifier {
/// #     fn notify(&mut self, value: &String) {}
/// # }
/// let notifier = MyNotifier;
///
/// let task = Task::with_notifications(
///     "process-request".to_string(),
///     Configuration::default(),
///     Some(notifier),
///     async {
///         // Process and return result
///         "processed".to_string()
///     }
/// );
/// ```
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
    pub fn priority(&self) -> priority::Priority {
        self.priority
    }

    // pub(crate) fn task_cancellation(&self) -> InFlightTaskCancellation {
    //     self.task.get_future().get_future().get_val(|cancellation| cancellation.clone())
    // }

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
    pub fn priority(&self) -> priority::Priority {
        self.priority
    }

    // pub(crate) fn task_cancellation(&self) -> InFlightTaskCancellation {
    //     self.task.get_future().get_future().get_val(|cancellation| cancellation.clone())
    // }

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
/// Provides information about the cancellation status of a running task.
///
/// `InFlightTaskCancellation` allows tasks to check if they have been cancelled
/// and optionally exit early. This is particularly useful for long-running tasks
/// that should stop processing when no longer needed.
///
/// # Examples
///
/// Tasks can check their cancellation status via the task-local variable:
///
/// ```no_run
/// // Inside a task's future, IS_CANCELLED can be accessed:
///  use some_executor::task::IS_CANCELLED;
///  if IS_CANCELLED.with(|c| c.expect("No task").is_cancelled()) {
///      // Clean up and exit early
///      return;
///  }
/// ```
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

    /// Returns `true` if the task has been cancelled.
    ///
    /// Tasks can use this method to check if they should stop processing and return early.
    /// Checking for cancellation is optional - tasks that don't check will continue to
    /// run until completion.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // Inside a long-running task:
    /// # use some_executor::task::IS_CANCELLED;
    /// for i in 0..1000 {
    ///      if IS_CANCELLED.with(|c| c.expect("No task").is_cancelled() ) {
    ///          println!("Task cancelled at iteration {}", i);
    ///          break;
    ///      }
    ///      // Do some work...
    ///  }
    /// ```
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
    - `configuration`: Configuration for the task.
    - `notifier`: An observer to notify when the task completes.  If there is no notifier, consider using [Self::without_notifications] instead.
    - `future`: The future to run.
    */
    pub fn with_notifications(
        label: String,
        configuration: Configuration,
        notifier: Option<N>,
        future: F,
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

    /// Returns the execution hint for this task.
    ///
    /// Hints provide guidance to executors about the expected behavior of the task,
    /// such as whether it will block or complete quickly.
    pub fn hint(&self) -> Hint {
        self.hint
    }

    /// Returns the human-readable label for this task.
    ///
    /// Labels are useful for debugging, monitoring, and identifying tasks in logs.
    pub fn label(&self) -> &str {
        self.label.as_ref()
    }

    /// Returns the priority of this task.
    ///
    /// Priority influences scheduling decisions when multiple tasks are ready to run.
    pub fn priority(&self) -> priority::Priority {
        self.priority
    }

    /// Returns the earliest time this task should be polled.
    ///
    /// Executors must not poll the task before this time. This can be used to
    /// implement delayed execution or rate limiting.
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, ConfigurationBuilder};
    /// use some_executor::Instant;
    /// use std::time::Duration;
    ///
    /// let config = ConfigurationBuilder::new()
    ///     .poll_after(Instant::now() + Duration::from_secs(5))
    ///     .build();
    ///
    /// let task = Task::without_notifications(
    ///     "delayed-task".to_string(),
    ///     config,
    ///     async { println!("This runs after 5 seconds"); },
    /// );
    /// ```
    pub fn poll_after(&self) -> crate::sys::Instant {
        self.poll_after
    }

    /// Returns the unique identifier for this task.
    ///
    /// Task IDs remain constant throughout the task's lifecycle and can be used
    /// for tracking, debugging, and correlation.
    pub fn task_id(&self) -> TaskID {
        self.task_id
    }

    /// Consumes the task and returns the underlying future.
    ///
    /// This is useful when you need direct access to the future, but note that
    /// you'll lose access to the task's metadata.
    pub fn into_future(self) -> F {
        self.future
    }

    /// Spawns the task onto an executor.
    ///
    /// This method transfers ownership of the task to the executor, which will poll it
    /// to completion. It returns a tuple containing:
    ///
    /// 1. A [`SpawnedTask`] that can be polled by the executor
    /// 2. A [`TypedObserver`] that can be used to await or check the task's completion
    ///
    /// # Arguments
    ///
    /// * `executor` - The executor that will run this task
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::any::Any;
    /// # use std::convert::Infallible;
    /// use std::future::Future;
    /// use std::pin::Pin;
    /// use some_executor::observer::{FinishedObservation, Observer, ObserverNotified, TypedObserver};
    /// use some_executor::task::{Task, Configuration};
    /// use some_executor::SomeExecutor;
    ///
    /// # struct MyExecutor;
    /// # impl SomeExecutor for MyExecutor {
    /// #     type ExecutorNotifier = std::convert::Infallible;
    /// #     fn spawn<F, N>(&mut self, task: Task<F, N>) -> impl some_executor::observer::Observer<Value = F::Output>
    /// #     where F: std::future::Future + Send + 'static, N: some_executor::observer::ObserverNotified<F::Output> + Send + 'static, F::Output: Send + 'static
    /// #     { todo!() as TypedObserver::<F::Output, Infallible> }
    /// #     fn spawn_async<F, N>(&mut self, task: Task<F, N>) -> impl std::future::Future<Output = impl some_executor::observer::Observer<Value = F::Output>>
    /// #     where F: std::future::Future + Send + 'static, N: some_executor::observer::ObserverNotified<F::Output> + Send + 'static, F::Output: Send + 'static  
    /// #     { async { todo!() as TypedObserver::<F::Output, Infallible>} }
    /// #     fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> { None }
    /// #     fn clone_box(&self) -> Box<dyn SomeExecutor<ExecutorNotifier = Self::ExecutorNotifier>> { todo!() }
    ///
    /// #     fn spawn_objsafe_async<'s>(&'s mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any + 'static + Send>> + 'static + Send>>, Box<dyn ObserverNotified<dyn Any + Send> + Send>>) -> Box<dyn Future<Output=Box<dyn Observer<Value=Box<dyn Any + Send>, Output=FinishedObservation<Box<dyn Any + Send>>> + Send>> + 's> {
    /// #        todo!()
    /// #     }
    ///
    /// #     fn spawn_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any + 'static + Send>> + 'static + Send>>, Box<dyn ObserverNotified<dyn Any + Send> + Send>>) -> Box<dyn Observer<Value=Box<dyn Any + Send>, Output=FinishedObservation<Box<dyn Any + Send>>> + Send> {
    /// #         todo!()
    /// #     }
    /// # }
    /// # async fn example() {
    /// let task = Task::without_notifications(
    ///     "compute".to_string(),
    ///     Configuration::default(),
    ///     async { 2 + 2 },
    /// );
    ///
    /// let mut executor = MyExecutor;
    /// let (spawned, observer) = task.spawn(&mut executor);
    ///
    /// // The task is now owned by the executor
    /// // Use the observer to check completion
    /// # }
    /// ```
    ///
    /// # Note
    ///
    /// When using this method, the `TASK_LOCAL_EXECUTOR` will be set to `None`.
    /// To spawn a task onto a local executor instead, use [`spawn_local`](Self::spawn_local).
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

    /// Spawns the task onto a local executor.
    ///
    /// Local executors are tied to a specific thread and cannot be sent across threads.
    /// This method is similar to [`spawn`](Self::spawn) but works with executors that
    /// are not `Send`.
    ///
    /// # Arguments
    ///
    /// * `executor` - The local executor that will run this task
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// 1. A [`SpawnedLocalTask`] that can be polled by the executor
    /// 2. A [`TypedObserver`] that can be used to await or check the task's completion
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration};
    /// # use some_executor::SomeLocalExecutor;
    /// # use some_executor::observer::{Observer, ObserverNotified, FinishedObservation, TypedObserver};
    /// # use std::any::Any;
    /// # use std::pin::Pin;
    /// # use std::future::Future;
    /// # use std::convert::Infallible;
    ///
    /// # struct MyLocalExecutor;
    /// # impl<'a> SomeLocalExecutor<'a> for MyLocalExecutor {
    /// #     type ExecutorNotifier = Infallible;
    /// #     fn spawn_local<F: Future, N: ObserverNotified<F::Output>>(&mut self, task: some_executor::task::Task<F, N>) -> impl Observer<Value = F::Output> where F: 'a, F::Output: 'static {
    /// #         todo!() as TypedObserver::<F::Output, Infallible>
    /// #     }
    /// #     fn spawn_local_async<F: Future, N: ObserverNotified<F::Output>>(&mut self, task: some_executor::task::Task<F, N>) -> impl Future<Output = impl Observer<Value = F::Output>> where F: 'a, F::Output: 'static {
    /// #         async { todo!() as TypedObserver::<F::Output, Infallible> }
    /// #     }
    /// #     fn spawn_local_objsafe(&mut self, task: some_executor::task::Task<Pin<Box<dyn Future<Output = Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Observer<Value = Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>> { todo!() }
    /// #     fn spawn_local_objsafe_async<'s>(&'s mut self, task: some_executor::task::Task<Pin<Box<dyn Future<Output = Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Future<Output = Box<dyn Observer<Value = Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>>> + 's> { Box::new(async { todo!() }) }
    /// #     fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> { None }
    /// # }
    /// let mut executor = MyLocalExecutor;
    ///
    /// let task = Task::without_notifications(
    ///     "local-work".to_string(),
    ///     Configuration::default(),
    ///     async {
    ///         // Can access thread-local data here
    ///         println!("Running on the local thread");
    ///     },
    /// );
    ///
    /// let (spawned, observer) = task.spawn_local(&mut executor);
    /// ```
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
    - `configuration`: Configuration for the task.
    - `future`: The future to run.


    # Details

    Use of this function is equivalent to calling [Task::with_notifications] with a None notifier.

    This function avoids the need to specify the type parameter to [Task].
    */
    pub fn without_notifications(label: String, configuration: Configuration, future: F) -> Self {
        Task::with_notifications(label, configuration, None, future)
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
        Self::with_notifications(label, configuration, notifier, Box::into_pin(future))
    }
}

/// Configuration options for spawning a task.
///
/// `Configuration` encapsulates the runtime preferences and scheduling hints for a task.
/// These settings help executors make informed decisions about how and when to run tasks.
///
/// # Examples
///
/// ## Using the default configuration
///
/// ```
/// use some_executor::task::{Task, Configuration};
///
/// let task = Task::without_notifications(
///     "simple".to_string(),
///     Configuration::default(),
///     async { "done" },
/// );
/// ```
///
/// ## Creating a custom configuration
///
/// ```
/// use some_executor::task::{Configuration, ConfigurationBuilder};
/// use some_executor::hint::Hint;
/// use some_executor::Priority;
///
/// // Build a configuration for a high-priority CPU task
/// let config = ConfigurationBuilder::new()
///     .hint(Hint::CPU)
///     .priority(Priority::unit_test())
///     .build();
///
/// // Or create directly
/// let config = Configuration::new(
///     Hint::CPU,
///     Priority::unit_test(),
///     some_executor::Instant::now()
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Configuration {
    hint: Hint,
    priority: priority::Priority,
    poll_after: crate::sys::Instant,
}

/// A builder for creating [`Configuration`] instances.
///
/// `ConfigurationBuilder` provides a fluent API for constructing task configurations
/// with optional settings. Any unspecified values will use sensible defaults.
///
/// # Examples
///
/// ```
/// use some_executor::task::ConfigurationBuilder;
/// use some_executor::hint::Hint;
/// use some_executor::Priority;
/// use some_executor::Instant;
/// use std::time::Duration;
///
/// // Build a configuration with specific settings
/// let config = ConfigurationBuilder::new()
///     .hint(Hint::CPU)
///     .priority(Priority::unit_test())
///     .poll_after(Instant::now() + Duration::from_secs(2))
///     .build();
///
/// // Use default builder - all fields will have default values
/// let default_config = ConfigurationBuilder::default().build();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ConfigurationBuilder {
    hint: Option<Hint>,
    priority: Option<Priority>,
    poll_after: Option<crate::sys::Instant>,
}

impl ConfigurationBuilder {
    /// Creates a new `ConfigurationBuilder` with no values set.
    ///
    /// All fields will be `None` until explicitly set via the builder methods.
    pub fn new() -> Self {
        ConfigurationBuilder {
            hint: None,
            priority: None,
            poll_after: None,
        }
    }

    /// Sets the execution hint for the task.
    ///
    /// Hints provide guidance to executors about the expected behavior of the task,
    /// allowing for more efficient scheduling decisions.
    ///
    /// # Arguments
    ///
    /// * `hint` - The execution hint for the task
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::ConfigurationBuilder;
    /// use some_executor::hint::Hint;
    ///
    /// let config = ConfigurationBuilder::new()
    ///     .hint(Hint::IO)
    ///     .build();
    /// ```
    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }

    /// Sets the priority for the task.
    ///
    /// Priority influences scheduling when multiple tasks are ready to run.
    /// Higher priority tasks are generally scheduled before lower priority ones.
    ///
    /// # Arguments
    ///
    /// * `priority` - The priority level for the task
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::ConfigurationBuilder;
    /// use some_executor::Priority;
    ///
    /// let config = ConfigurationBuilder::new()
    ///     .priority(Priority::unit_test())
    ///     .build();
    /// ```
    pub fn priority(mut self, priority: priority::Priority) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Sets the earliest time the task should be polled.
    ///
    /// This can be used to delay task execution or implement scheduled tasks.
    /// Executors will not poll the task before this time.
    ///
    /// # Arguments
    ///
    /// * `poll_after` - The instant after which the task can be polled
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::ConfigurationBuilder;
    /// use some_executor::Instant;
    /// use std::time::Duration;
    ///
    /// // Delay task execution by 5 seconds
    /// let config = ConfigurationBuilder::new()
    ///     .poll_after(Instant::now() + Duration::from_secs(5))
    ///     .build();
    /// ```
    pub fn poll_after(mut self, poll_after: crate::sys::Instant) -> Self {
        self.poll_after = Some(poll_after);
        self
    }

    /// Builds the final [`Configuration`] instance.
    ///
    /// Any unset values will use their defaults:
    /// - `hint`: `Hint::default()`
    /// - `priority`: `Priority::Unknown`
    /// - `poll_after`: `Instant::now()`
    pub fn build(self) -> Configuration {
        Configuration {
            hint: self.hint.unwrap_or_default(),
            priority: self.priority.unwrap_or(priority::Priority::Unknown),
            poll_after: self.poll_after.unwrap_or_else(crate::sys::Instant::now),
        }
    }
}

impl Configuration {
    /// Creates a new `Configuration` with the specified values.
    ///
    /// This is an alternative to using [`ConfigurationBuilder`] when you have all
    /// values available upfront.
    ///
    /// # Arguments
    ///
    /// * `hint` - The execution hint for the task
    /// * `priority` - The priority level for the task
    /// * `poll_after` - The earliest time the task should be polled
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::Configuration;
    /// use some_executor::hint::Hint;
    /// use some_executor::Priority;
    /// use some_executor::Instant;
    ///
    /// let config = Configuration::new(
    ///     Hint::CPU,
    ///     Priority::unit_test(),
    ///     Instant::now()
    /// );
    /// ```
    pub fn new(hint: Hint, priority: priority::Priority, poll_after: crate::sys::Instant) -> Self {
        Configuration {
            hint,
            priority,
            poll_after,
        }
    }
}

/// Object-safe type-erased wrapper for [`SpawnedLocalTask`].
///
/// This trait allows executors to work with spawned local tasks without knowing their
/// concrete future type. It provides all the essential operations needed to poll and
/// query task metadata in a type-erased manner.
///
/// # Usage
///
/// This trait is primarily used internally by executors that need to store heterogeneous
/// collections of local tasks. The trait methods mirror those of [`SpawnedLocalTask`].
///
/// # Implementation
///
/// This trait is automatically implemented for all [`SpawnedLocalTask`] instances.
pub trait DynLocalSpawnedTask<Executor> {
    /// Polls the task with its associated executor.
    ///
    /// # Arguments
    ///
    /// * `cx` - The waker context for async execution
    /// * `executor` - The local executor that owns this task
    /// * `some_executor` - Optional executor for spawning new tasks from within this task
    fn poll<'executor>(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        executor: &'executor mut Executor,
        some_executor: Option<Box<(dyn SomeExecutor<ExecutorNotifier = Infallible> + 'static)>>,
    ) -> std::task::Poll<()>;

    /// Returns the earliest time this task should be polled.
    fn poll_after(&self) -> crate::sys::Instant;

    /// Returns the task's label.
    fn label(&self) -> &str;

    /// Returns the task's unique identifier.
    fn task_id(&self) -> TaskID;

    /// Returns the task's execution hint.
    fn hint(&self) -> Hint;

    /// Returns the task's priority.
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

/// Object-safe type-erased wrapper for [`SpawnedTask`].
///
/// This trait allows executors to work with spawned tasks without knowing their
/// concrete future type. It provides all the essential operations needed to poll and
/// query task metadata in a type-erased manner.
///
/// # Type Parameters
///
/// * `LocalExecutorType` - The type of local executor that may be provided during polling.
///   Use [`Infallible`] if no local executor is needed.
///
/// # Usage
///
/// This trait is primarily used internally by executors that need to store heterogeneous
/// collections of tasks. The trait is `Send + Debug`, making it suitable for multi-threaded
/// executors.
///
/// # Examples
///
/// ```
/// use some_executor::task::DynSpawnedTask;
/// use std::convert::Infallible;
///
/// // Store a collection of type-erased tasks
/// let tasks: Vec<Box<dyn DynSpawnedTask<Infallible>>> = vec![];
/// ```
///
/// # Implementation
///
/// This trait is automatically implemented for all [`SpawnedTask`] instances where the
/// future and notifier types are `Send`.
pub trait DynSpawnedTask<LocalExecutorType>: Send + Debug {
    /// Polls the task, optionally with a local executor context.
    ///
    /// # Arguments
    ///
    /// * `cx` - The waker context for async execution
    /// * `local_executor` - Optional local executor for thread-local operations
    fn poll<'l>(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        local_executor: Option<&mut LocalExecutorType>,
    ) -> std::task::Poll<()>
    where
        LocalExecutorType: SomeLocalExecutor<'l>;

    /// Returns the earliest time this task should be polled.
    fn poll_after(&self) -> crate::sys::Instant;

    /// Returns the task's label.
    fn label(&self) -> &str;

    /// Returns the task's unique identifier.
    fn task_id(&self) -> TaskID;

    /// Returns the task's execution hint.
    fn hint(&self) -> Hint;

    /// Returns the task's priority.
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

/// A trivial future that completes immediately with `()`.
///
/// `DefaultFuture` is useful for creating default task instances or for testing
/// purposes where you need a simple, non-blocking future.
///
/// # Examples
///
/// ```
/// use some_executor::task::{DefaultFuture, Task};
/// use std::convert::Infallible;
/// use std::task::Poll;
/// use std::future::Future;
///
/// // DefaultFuture always returns Ready(())
/// # use std::task::Context;
/// # use std::pin::Pin;
/// # fn make_waker() -> std::task::Waker {
/// #     use std::task::{RawWaker, RawWakerVTable, Waker};
/// #     unsafe fn no_op(_: *const ()) {}
/// #     unsafe fn clone(_: *const ()) -> RawWaker {
/// #         RawWaker::new(std::ptr::null(), &VTABLE)
/// #     }
/// #     const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
/// #     unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
/// # }
/// # let waker = make_waker();
/// # let mut cx = Context::from_waker(&waker);
/// let mut future = DefaultFuture;
/// assert_eq!(Pin::new(&mut future).poll(&mut cx), Poll::Ready(()));
///
/// // Can be used to create a default task
/// let task: Task<DefaultFuture, Infallible> = Task::default();
/// ```
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
            Configuration::default(),
            None,
            DefaultFuture,
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
        Task::with_notifications("".to_string(), Configuration::default(), None, future)
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

impl From<InFlightTaskCancellation> for bool {
    fn from(val: InFlightTaskCancellation) -> Self {
        val.0.load(std::sync::atomic::Ordering::Relaxed)
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
    use crate::{SomeExecutor, SomeLocalExecutor, task_local};
    use std::any::Any;
    use std::convert::Infallible;
    use std::future::Future;
    use std::pin::Pin;

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_create_task() {
        let task: Task<_, Infallible> =
            Task::with_notifications("test".to_string(), Default::default(), None, async {});
        assert_eq!(task.label(), "test");
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_create_no_notify() {
        let t = Task::without_notifications("test".to_string(), Default::default(), async {});
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
