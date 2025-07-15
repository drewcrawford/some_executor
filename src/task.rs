// SPDX-License-Identifier: MIT OR Apache-2.0

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

mod config;
mod dyn_spawned;
mod spawned;
mod task_local;

// Re-exports from submodules
pub use dyn_spawned::{DynLocalSpawnedTask, DynSpawnedTask};
pub use spawned::{SpawnedLocalTask, SpawnedStaticTask, SpawnedTask};
pub use task_local::{
    IS_CANCELLED, InFlightTaskCancellation, TASK_EXECUTOR, TASK_ID, TASK_LABEL,
    TASK_LOCAL_EXECUTOR, TASK_PRIORITY, TASK_STATIC_EXECUTOR,
};

use crate::dyn_observer_notified::{ObserverNotifiedErased, ObserverNotifiedErasedLocal};
use crate::hint::Hint;
use crate::observer::{
    ExecutorNotified, Observer, ObserverNotified, TypedObserver, observer_channel,
};
use crate::{ObjSafeStaticTask, Priority, SomeExecutor, SomeLocalExecutor, SomeStaticExecutor};
use std::any::Any;
use std::convert::Infallible;
use std::fmt::Debug;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::atomic::AtomicU64;
use std::task::Poll;

// Type aliases for complex types to satisfy clippy::type_complexity warnings

/// Type alias for a boxed future that outputs boxed Any and is Send + 'static
type BoxedSendFuture =
    Pin<Box<dyn Future<Output = Box<dyn Any + 'static + Send>> + 'static + Send>>;

/// Type alias for a boxed observer notifier that handles Send Any values
type BoxedSendObserverNotifier = Box<dyn ObserverNotified<dyn Any + Send> + Send>;

/// Type alias for a Task that can be used with object-safe spawning
type ObjSafeTask = Task<BoxedSendFuture, BoxedSendObserverNotifier>;

/// Type alias for a boxed future that outputs boxed Any (non-Send)
type BoxedLocalFuture = Pin<Box<dyn Future<Output = Box<dyn Any + 'static>> + 'static>>;

/// Type alias for a boxed observer notifier that handles Any values (non-Send)
type BoxedLocalObserverNotifier = Box<dyn ObserverNotified<dyn Any + 'static>>;

/// Type alias for a Task that can be used with local object-safe spawning
type ObjSafeLocalTask = Task<BoxedLocalFuture, BoxedLocalObserverNotifier>;

/// Type alias for the result of spawning a task onto an executor
/// Returns a tuple of (SpawnedTask, TypedObserver)
type SpawnResult<F, N, Executor> = (
    SpawnedTask<F, N, Executor>,
    TypedObserver<<F as Future>::Output, <Executor as SomeExecutor>::ExecutorNotifier>,
);

/// Type alias for the result of spawning a task onto a local executor
/// Returns a tuple of (SpawnedLocalTask, TypedObserver)
type SpawnLocalResult<'a, F, N, Executor> = (
    SpawnedLocalTask<F, N, Executor>,
    TypedObserver<<F as Future>::Output, <Executor as SomeLocalExecutor<'a>>::ExecutorNotifier>,
);

/// Type alias for the result of spawning a task on a static executor
/// Returns a tuple with a spawned static task and observer
type SpawnStaticResult<F, N, Executor> = (
    SpawnedStaticTask<F, N, Executor>,
    TypedObserver<<F as Future>::Output, <Executor as SomeStaticExecutor>::ExecutorNotifier>,
);

/// Type alias for the result of spawning a task using object-safe method
/// Returns a tuple with a boxed executor notifier
type SpawnObjSafeResult<F, N, Executor> = (
    SpawnedTask<F, N, Executor>,
    TypedObserver<<F as Future>::Output, Box<dyn ExecutorNotified + Send>>,
);

/// Type alias for the result of spawning a local task using object-safe method
/// Returns a tuple with a boxed executor notifier
type SpawnLocalObjSafeResult<F, N, Executor> = (
    SpawnedLocalTask<F, N, Executor>,
    TypedObserver<<F as Future>::Output, Box<dyn ExecutorNotified>>,
);

/// Type alias for the result of spawning a static task using object-safe method
/// Returns a tuple with a boxed executor notifier
type SpawnStaticObjSafeResult<F, N, Executor> = (
    SpawnedStaticTask<F, N, Executor>,
    TypedObserver<<F as Future>::Output, Box<dyn ExecutorNotified>>,
);

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
            hint: configuration.hint(),
            poll_after: configuration.poll_after(),
            priority: configuration.priority(),
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
    ) -> SpawnResult<F, N, Executor> {
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
        let spawned_task = spawned::SpawnedTask {
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
    ) -> SpawnLocalResult<'executor, F, N, Executor> {
        let cancellation = InFlightTaskCancellation::default();
        let task_id = self.task_id();
        let (sender, receiver) = observer_channel(
            self.notifier.take(),
            executor.executor_notifier(),
            cancellation.clone(),
            task_id,
        );
        let spawned_task = spawned::SpawnedLocalTask {
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

    /// Spawns the task onto a static executor.
    ///
    /// Static executors handle futures that are `'static` but not necessarily `Send`.
    /// This is useful for thread-local executors that work with static data but don't
    /// need to cross thread boundaries.
    ///
    /// # Arguments
    ///
    /// * `executor` - The static executor that will run this task
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// 1. A [`SpawnedStaticTask`] that can be polled by the executor
    /// 2. A [`TypedObserver`] that can be used to await or check the task's completion
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration};
    /// # use some_executor::SomeStaticExecutor;
    /// # use some_executor::observer::{Observer, ObserverNotified, FinishedObservation, TypedObserver};
    /// # use std::any::Any;
    /// # use std::pin::Pin;
    /// # use std::future::Future;
    /// # use std::convert::Infallible;
    ///
    /// # struct MyStaticExecutor;
    /// # impl SomeStaticExecutor for MyStaticExecutor {
    /// #     type ExecutorNotifier = Box<dyn some_executor::observer::ExecutorNotified>;
    /// #     fn spawn_static<F: Future, N: ObserverNotified<F::Output>>(&mut self, task: some_executor::task::Task<F, N>) -> impl Observer<Value = F::Output> where F: 'static, F::Output: 'static {
    /// #         todo!() as TypedObserver::<F::Output, Box<dyn some_executor::observer::ExecutorNotified>>
    /// #     }
    /// #     fn spawn_static_async<F: Future, N: ObserverNotified<F::Output>>(&mut self, task: some_executor::task::Task<F, N>) -> impl Future<Output = impl Observer<Value = F::Output>> where F: 'static, F::Output: 'static {
    /// #         async { todo!() as TypedObserver::<F::Output, Box<dyn some_executor::observer::ExecutorNotified>> }
    /// #     }
    /// #     fn spawn_static_objsafe(&mut self, task: some_executor::task::Task<Pin<Box<dyn Future<Output = Box<dyn Any + 'static>> + 'static>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Observer<Value = Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>> { todo!() }
    /// #     fn spawn_static_objsafe_async<'s>(&'s mut self, task: some_executor::task::Task<Pin<Box<dyn Future<Output = Box<dyn Any + 'static>> + 'static>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Future<Output = Box<dyn Observer<Value = Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>>> + 's> { Box::new(async { todo!() }) }
    /// #     fn clone_box(&self) -> Box<some_executor::DynStaticExecutor> { todo!() }
    /// #     fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> { None }
    /// # }
    /// let mut executor = MyStaticExecutor;
    ///
    /// let task = Task::without_notifications(
    ///     "static-work".to_string(),
    ///     Configuration::default(),
    ///     async {
    ///         // Can access static data here
    ///         println!("Running on static executor");
    ///     },
    /// );
    ///
    /// let (spawned, observer) = task.spawn_static(&mut executor);
    /// ```
    pub fn spawn_static<Executor: SomeStaticExecutor>(
        mut self,
        executor: &mut Executor,
    ) -> SpawnStaticResult<F, N, Executor> {
        let cancellation = InFlightTaskCancellation::default();
        let task_id = self.task_id();
        let (sender, receiver) = observer_channel(
            self.notifier.take(),
            executor.executor_notifier(),
            cancellation.clone(),
            task_id,
        );
        let spawned_task = spawned::SpawnedStaticTask {
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
    ) -> SpawnObjSafeResult<F, N, Executor> {
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
        let spawned_task = spawned::SpawnedTask {
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
    ) -> SpawnLocalObjSafeResult<F, N, Executor> {
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
        let spawned_task = spawned::SpawnedLocalTask {
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

    /// Spawns the task onto a static executor using object-safe method.
    ///
    /// This method is similar to [`spawn_static`](Self::spawn_static) but uses type erasure
    /// to support object-safe trait usage.
    ///
    /// # Arguments
    ///
    /// * `executor` - The static executor that will run this task
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// 1. A [`SpawnedStaticTask`] that can be polled by the executor
    /// 2. A [`TypedObserver`] that can be used to await or check the task's completion
    pub fn spawn_static_objsafe<Executor: SomeStaticExecutor>(
        mut self,
        executor: &mut Executor,
    ) -> SpawnStaticObjSafeResult<F, N, Executor> {
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
        let spawned_task = spawned::SpawnedStaticTask {
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
    pub fn into_objsafe(self) -> ObjSafeTask
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

    /**
    Converts this task into one suitable for spawn_local_objsafe
    */
    pub fn into_objsafe_local(self) -> ObjSafeLocalTask
    where
        N: ObserverNotified<F::Output>,
        F::Output: 'static + Unpin,
        F: 'static,
    {
        let notifier = self.notifier.map(|n| {
            Box::new(ObserverNotifiedErasedLocal::new(n))
                as Box<dyn ObserverNotified<dyn Any + 'static>>
        });
        Task::new_objsafe_local(
            self.label,
            Box::new(async move { Box::new(self.future.await) as Box<dyn Any + 'static> }),
            Configuration::new(self.hint, self.priority, self.poll_after),
            notifier,
        )
    }

    /// Converts this task into one suitable for spawn_static_objsafe
    pub fn into_objsafe_static(self) -> ObjSafeStaticTask
    where
        N: ObserverNotified<F::Output>,
        F::Output: 'static + Unpin,
        F: 'static,
    {
        let notifier = self.notifier.map(|n| {
            Box::new(ObserverNotifiedErasedLocal::new(n))
                as Box<dyn ObserverNotified<dyn Any + 'static>>
        });
        Task::new_objsafe_static(
            self.label,
            Box::new(async move { Box::new(self.future.await) as Box<dyn Any + 'static> }),
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

// Methods for tasks that output ()
impl<F: Future<Output = ()>, N> Task<F, N> {
    /// Spawns the task onto the current executor and detaches the observer.
    ///
    /// This is a convenience method for fire-and-forget tasks that don't return a value.
    /// The task will be spawned using [`current_executor`](crate::current_executor::current_executor)
    /// and the observer will be automatically detached.
    ///
    /// # Requirements
    ///
    /// - The future must output `()`
    /// - The future must be `Send + 'static`
    /// - The notifier must be `Send + 'static`
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration};
    ///
    /// # async fn example() {
    /// let task = Task::without_notifications(
    ///     "background-work".to_string(),
    ///     Configuration::default(),
    ///     async {
    ///         println!("Running in background");
    ///     },
    /// );
    ///
    /// // Spawn and forget
    /// task.spawn_current();
    /// # }
    /// ```
    pub fn spawn_current(self)
    where
        F: Send + 'static,
        N: ObserverNotified<()> + Send + 'static,
    {
        let mut executor = crate::current_executor::current_executor();
        executor.spawn(self).detach();
    }

    /// Spawns the task onto the thread-local executor and detaches the observer.
    ///
    /// This is a convenience method for fire-and-forget tasks that don't return a value
    /// and may not be `Send`. The task will be spawned using the thread-local executor
    /// and the observer will be automatically detached.
    ///
    /// # Requirements
    ///
    /// - The future must output `()`
    /// - The future must be `'static` (but doesn't need to be `Send`)
    /// - The notifier must implement `ObserverNotified<()>`
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration};
    /// use std::rc::Rc;
    ///
    /// # fn example() {
    /// // Rc is !Send
    /// let data = Rc::new(42);
    /// let data_clone = data.clone();
    ///
    /// let task = Task::without_notifications(
    ///     "local-work".to_string(),
    ///     Configuration::default(),
    ///     async move {
    ///         println!("Local data: {}", data_clone);
    ///     },
    /// );
    ///
    /// // Spawn on thread-local executor
    /// task.spawn_thread_local();
    /// # }
    /// ```
    pub fn spawn_thread_local(self)
    where
        F: 'static,
        N: ObserverNotified<()> + 'static,
    {
        todo!("Not yet implemented");
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

impl
    Task<
        Pin<Box<dyn Future<Output = Box<dyn Any + 'static>> + 'static>>,
        Box<dyn ObserverNotified<dyn Any + 'static>>,
    >
{
    /**
            Creates a new local objsafe future
    */
    pub fn new_objsafe_local(
        label: String,
        future: Box<dyn Future<Output = Box<dyn Any + 'static>> + 'static>,
        configuration: Configuration,
        notifier: Option<Box<dyn ObserverNotified<dyn Any + 'static>>>,
    ) -> Self {
        Self::with_notifications(label, configuration, notifier, Box::into_pin(future))
    }

    /// Creates a new task suitable for static objsafe spawning.
    ///
    /// This constructor is used internally by [`into_objsafe_static`](Task::into_objsafe_static)
    /// to create tasks that can be spawned on static executors using object-safe methods.
    pub fn new_objsafe_static(
        label: String,
        future: Box<dyn Future<Output = Box<dyn Any + 'static>> + 'static>,
        configuration: Configuration,
        notifier: Option<Box<dyn ObserverNotified<dyn Any + 'static>>>,
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
pub use config::{Configuration, ConfigurationBuilder};

/* boilerplates

configuration - default

*/

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
        #[allow(unused)]
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

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_into_objsafe_local_non_send() {
        use crate::task::Configuration;
        use std::rc::Rc;

        // Create a non-Send type (Rc cannot be sent across threads)
        let non_send_data = Rc::new(42);

        // Create a future that captures the non-Send data
        let non_send_future = async move {
            let _captured = non_send_data; // This makes the future !Send
            "result"
        };

        // Create a task with the non-Send future
        let task = Task::without_notifications(
            "non-send-task".to_string(),
            Configuration::default(),
            non_send_future,
        );

        // Convert to objsafe local task - this should work because we don't require Send
        let objsafe_task = task.into_objsafe_local();

        // Verify the task properties are preserved
        assert_eq!(objsafe_task.label(), "non-send-task");
        assert_eq!(objsafe_task.hint(), crate::hint::Hint::default());
        assert_eq!(objsafe_task.priority(), crate::Priority::Unknown);

        // This would not compile with into_objsafe() because the future is !Send:
        // let _would_fail = task.into_objsafe(); // <- This line would cause compile error
        // but it works with into_objsafe_local() because we don't require Send bounds
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_nested_spawn_thread_local_no_panic() {
        // This test verifies that the nested submission bug is fixed
        // The key is that it should not panic with BorrowMutError anymore

        use crate::thread_executor::thread_local_executor;
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };

        let nested_call_worked = Arc::new(AtomicBool::new(false));
        let nested_call_worked_clone = nested_call_worked.clone();

        // This should not panic with BorrowMutError
        thread_local_executor(|_executor_rc1| {
            // This nested call should work now
            thread_local_executor(|_executor_rc2| {
                // If we get here without panicking, the fix worked!
                nested_call_worked_clone.store(true, Ordering::Relaxed);
            });
        });

        // If we reach here, the BorrowMutError is fixed
        assert!(
            nested_call_worked.load(Ordering::Relaxed),
            "Nested call should have succeeded"
        );
    }
}
