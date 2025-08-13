// SPDX-License-Identifier: MIT OR Apache-2.0

//! Task-local storage and cancellation support for async tasks.
//!
//! This module provides task-local variables that are scoped to individual async tasks,
//! similar to thread-local storage but for the async world. Task-locals allow you to
//! store context that is accessible throughout a task's execution without explicitly
//! passing it through function parameters.
//!
//! # Task-Locals vs Thread-Locals
//!
//! While thread-local storage is scoped to OS threads, task-local storage is scoped to
//! async tasks. Key differences include:
//!
//! - **Scope**: Task-locals are only accessible within the async task that set them
//! - **Inheritance**: Task-locals are not inherited by spawned child tasks
//! - **Lifetime**: Task-locals are automatically cleaned up when the task completes
//! - **Thread-agnostic**: A task may move between threads, but its task-locals follow it
//!
//! # Built-in Task-Locals
//!
//! The executor framework provides several built-in task-locals:
//!
//! - [`TASK_LABEL`]: A debugging label for the current task
//! - [`TASK_ID`]: A unique identifier for the current task
//! - [`TASK_PRIORITY`]: The scheduling priority of the current task
//! - [`IS_CANCELLED`]: Cancellation status that tasks can check to exit early
//! - [`TASK_EXECUTOR`]: Reference to the executor running the task
//!
//! # Cancellation
//!
//! This module also provides the cancellation mechanism through [`InFlightTaskCancellation`].
//! Tasks can cooperatively check if they've been cancelled and exit early to avoid
//! unnecessary work.
//!
//! # Examples
//!
//! ## Accessing Built-in Task-Locals
//!
//! ```
//! use some_executor::task::{Task, Configuration, TASK_LABEL, TASK_ID};
//! use some_executor::SomeExecutor;
//!
//! # fn example() {
//! let task = Task::without_notifications(
//!     "my-task".to_string(),
//!     Configuration::default(),
//!     async {
//!         // Access task label
//!         TASK_LABEL.with(|label| {
//!             // In a real task context, this would have a value
//!             if let Some(name) = label {
//!                 println!("Running task: {}", name);
//!             }
//!         });
//!         
//!         // Access task ID
//!         TASK_ID.with(|id| {
//!             // In a real task context, this would have a value
//!             if let Some(task_id) = id {
//!                 println!("Task ID: {:?}", task_id);
//!             }
//!         });
//!     },
//! );
//! # }
//! ```
//!
//! ## Checking for Cancellation
//!
//! ```
//! use some_executor::task::{Task, Configuration, IS_CANCELLED};
//!
//! # fn example() {
//! let task = Task::without_notifications(
//!     "cancellable-task".to_string(),
//!     Configuration::default(),
//!     async {
//!         for i in 0..10 {
//!             // Check if we've been cancelled
//!             if IS_CANCELLED.with(|c| c.map(|c| c.is_cancelled()).unwrap_or(false)) {
//!                 println!("Task cancelled at iteration {}", i);
//!                 return Err("Cancelled");
//!             }
//!             
//!             // Simulate some work - in real code you'd await some async operation here
//!             // For example: some_async_work().await;
//!         }
//!         Ok("Completed")
//!     },
//! );
//! # }
//! ```

use super::TaskID;
use crate::observer::ExecutorNotified;
use crate::{DynExecutor, Priority, SomeLocalExecutor, SomeStaticExecutor, task_local};
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

// Type aliases for complex types to satisfy clippy::type_complexity warnings

/// Type alias for a thread-local executor that can handle local tasks
type ThreadLocalExecutor = RefCell<
    Option<Box<dyn SomeLocalExecutor<'static, ExecutorNotifier = Box<dyn ExecutorNotified>>>>,
>;

/// Provides information about the cancellation status of a running task.
///
/// `InFlightTaskCancellation` allows tasks to check if they have been cancelled
/// and optionally exit early. This is particularly useful for long-running tasks
/// that should stop processing when no longer needed.
///
/// The cancellation mechanism is cooperative - tasks must explicitly check their
/// cancellation status and decide how to handle it. Tasks that don't check will
/// continue running until completion.
///
/// # Thread Safety
///
/// `InFlightTaskCancellation` uses atomic operations internally and is safe to
/// share across threads. Multiple observers can hold references to the same
/// cancellation token.
///
/// # Examples
///
/// ## Basic Cancellation Check
///
/// ```
/// use some_executor::task::{Task, Configuration, IS_CANCELLED};
///
/// # fn example() {
/// let task = Task::without_notifications(
///     "check-cancel".to_string(),
///     Configuration::default(),
///     async {
///         // Check cancellation status
///         let is_cancelled = IS_CANCELLED.with(|c| {
///             c.map(|cancel| cancel.is_cancelled()).unwrap_or(false)
///         });
///         
///         if is_cancelled {
///             println!("Task was cancelled");
///         } else {
///             println!("Task is running normally");
///         }
///     },
/// );
/// # }
/// ```
///
/// ## Periodic Cancellation Checks in Loops
///
/// ```
/// use some_executor::task::{Task, Configuration, IS_CANCELLED};
///
/// async fn process_items(items: Vec<i32>) -> Result<i32, &'static str> {
///     let mut sum = 0;
///     
///     for (i, item) in items.iter().enumerate() {
///         // Check cancellation every 100 items
///         if i % 100 == 0 {
///             if IS_CANCELLED.with(|c| c.map(|c| c.is_cancelled()).unwrap_or(false)) {
///                 println!("Processing cancelled at item {}", i);
///                 return Err("Cancelled");
///             }
///         }
///         
///         // Simulate processing
///         sum += item;
///     }
///     
///     Ok(sum)
/// }
///
/// # fn example() {
/// let task = Task::without_notifications(
///     "process-items".to_string(),
///     Configuration::default(),
///     async {
///         let items = (0..1000).collect();
///         process_items(items).await
///     },
/// );
/// # }
/// ```
#[derive(Debug, Clone)]
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
    /// This method uses relaxed memory ordering for performance, which is sufficient
    /// for cancellation checks.
    ///
    /// # Examples
    ///
    /// ## Simple Cancellation Check
    ///
    /// ```
    /// use some_executor::task::InFlightTaskCancellation;
    ///
    /// let cancellation = InFlightTaskCancellation::default();
    /// assert!(!cancellation.is_cancelled());
    ///
    /// // After cancellation (normally done by the observer)
    /// let cancellation = InFlightTaskCancellation::from(true);
    /// assert!(cancellation.is_cancelled());
    /// ```
    ///
    /// ## Within a Task Context
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration, IS_CANCELLED};
    ///
    /// async fn long_running_operation() -> Result<(), &'static str> {
    ///     for i in 0..1000 {
    ///         // Check cancellation periodically
    ///         if IS_CANCELLED.with(|c| c.map(|c| c.is_cancelled()).unwrap_or(false)) {
    ///             println!("Operation cancelled at iteration {}", i);
    ///             return Err("Cancelled");
    ///         }
    ///         
    ///         // Simulate work
    ///         if i % 100 == 0 {
    ///             println!("Processing item {}", i);
    ///         }
    ///     }
    ///     Ok(())
    /// }
    ///
    /// # fn example() {
    /// let task = Task::without_notifications(
    ///     "long-operation".to_string(),
    ///     Configuration::default(),
    ///     long_running_operation(),
    /// );
    /// # }
    /// ```
    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

task_local! {
    /// Provides a debugging label to identify the current task.
    ///
    /// This label is set when a task is created and can be used for debugging,
    /// logging, and monitoring purposes. The label follows the task as it
    /// executes, even if it moves between threads.
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration, TASK_LABEL};
    ///
    /// # fn example() {
    /// let task = Task::without_notifications(
    ///     "data-processor".to_string(),
    ///     Configuration::default(),
    ///     async {
    ///         TASK_LABEL.with(|label| {
    ///             if let Some(name) = label {
    ///                 println!("Starting task: {}", name);
    ///             }
    ///         });
    ///         // Process data...
    ///     },
    /// );
    /// # }
    /// ```
    pub static const TASK_LABEL: String;

    /// Provides the scheduling priority for the current task.
    ///
    /// The priority can influence how the executor schedules the task relative
    /// to other tasks. Higher priority tasks may be scheduled more frequently
    /// or given preference during execution.
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration, TASK_PRIORITY};
    /// use some_executor::Priority;
    ///
    /// # fn example() {
    /// let task = Task::without_notifications(
    ///     "priority-check".to_string(),
    ///     Configuration::default(),
    ///     async {
    ///         TASK_PRIORITY.with(|priority| {
    ///             if let Some(p) = priority {
    ///                 println!("Task priority: {:?}", p);
    ///             }
    ///         });
    ///     },
    /// );
    /// # }
    /// ```
    pub static const TASK_PRIORITY: Priority;

    /// Provides a mechanism for tasks to determine if they have been cancelled.
    ///
    /// Tasks can cooperatively check this value to determine if they should stop
    /// running. This enables graceful shutdown of long-running operations when
    /// cancellation is requested.
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration, IS_CANCELLED};
    ///
    /// async fn interruptible_work() -> Result<(), &'static str> {
    ///     for _ in 0..100 {
    ///         // Check for cancellation
    ///         if IS_CANCELLED.with(|c| c.map(|c| c.is_cancelled()).unwrap_or(false)) {
    ///             return Err("Cancelled");
    ///         }
    ///
    ///         // Do a unit of work
    ///         // In real code: some_async_operation().await;
    ///     }
    ///     Ok(())
    /// }
    ///
    /// # fn example() {
    /// let task = Task::without_notifications(
    ///     "interruptible".to_string(),
    ///     Configuration::default(),
    ///     interruptible_work(),
    /// );
    /// # }
    /// ```
    pub static const IS_CANCELLED: InFlightTaskCancellation;

    /// Provides a unique identifier for the current task.
    ///
    /// Each task is assigned a globally unique ID when created. This ID can be
    /// used for tracking, logging, and debugging purposes.
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration, TASK_ID};
    ///
    /// # fn example() {
    /// let task = Task::without_notifications(
    ///     "tracked-task".to_string(),
    ///     Configuration::default(),
    ///     async {
    ///         TASK_ID.with(|id| {
    ///             if let Some(task_id) = id {
    ///                 println!("Task {} is running", task_id.to_u64());
    ///             }
    ///         });
    ///     },
    /// );
    /// # }
    /// ```
    pub static const TASK_ID: TaskID;

    /// Provides access to the executor running the current task.
    ///
    /// This allows tasks to spawn additional work on the same executor without
    /// needing to pass the executor explicitly through function parameters.
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration, TASK_EXECUTOR};
    ///
    /// # fn example() {
    /// let task = Task::without_notifications(
    ///     "spawner".to_string(),
    ///     Configuration::default(),
    ///     async {
    ///         TASK_EXECUTOR.with(|executor| {
    ///             if let Some(exec) = executor {
    ///                 // Could spawn additional tasks here
    ///                 println!("Have access to executor");
    ///             }
    ///         });
    ///     },
    /// );
    /// # }
    /// ```
    pub static const TASK_EXECUTOR: Option<Box<DynExecutor>>;
}

task_local! {
    /// Provides a static executor local to the current task.
    ///
    /// This is similar to [`TASK_EXECUTOR`] but specifically for static executors
    /// that can handle `!Send` futures. Static executors are bound to a single
    /// thread and can execute futures that don't implement `Send`.
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration, TASK_STATIC_EXECUTOR};
    /// use std::rc::Rc;
    ///
    /// # fn example() {
    /// let task = Task::without_notifications(
    ///     "static-executor-task".to_string(),
    ///     Configuration::default(),
    ///     async {
    ///         // Rc is !Send, so it requires a static executor
    ///         let data = Rc::new(42);
    ///
    ///         TASK_STATIC_EXECUTOR.with(|executor| {
    ///             if let Some(exec) = executor {
    ///                 // Could spawn !Send futures here
    ///                 println!("Have access to static executor");
    ///             }
    ///         });
    ///
    ///         // Use the !Send data
    ///         assert_eq!(*data, 42);
    ///     },
    /// );
    /// # }
    /// ```
    pub static const TASK_STATIC_EXECUTOR: Option<Box<dyn SomeStaticExecutor<ExecutorNotifier = Box<dyn ExecutorNotified>>>>;
}

thread_local! {
    /// Thread-local storage for the current thread's local executor.
    ///
    /// This allows tasks to access a local executor that can handle `!Send` futures
    /// on the current thread. Unlike task-local executors, this is scoped to the
    /// OS thread rather than the async task.
    pub static TASK_LOCAL_EXECUTOR: ThreadLocalExecutor = RefCell::new(None);
}

impl Default for InFlightTaskCancellation {
    /// Creates a new `InFlightTaskCancellation` in the non-cancelled state.
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::InFlightTaskCancellation;
    ///
    /// let cancellation = InFlightTaskCancellation::default();
    /// assert!(!cancellation.is_cancelled());
    /// ```
    fn default() -> Self {
        InFlightTaskCancellation(Arc::new(AtomicBool::new(false)))
    }
}

impl From<bool> for InFlightTaskCancellation {
    /// Creates an `InFlightTaskCancellation` with the specified initial state.
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::InFlightTaskCancellation;
    ///
    /// let not_cancelled = InFlightTaskCancellation::from(false);
    /// assert!(!not_cancelled.is_cancelled());
    ///
    /// let cancelled = InFlightTaskCancellation::from(true);
    /// assert!(cancelled.is_cancelled());
    /// ```
    fn from(value: bool) -> Self {
        InFlightTaskCancellation(Arc::new(AtomicBool::new(value)))
    }
}

impl From<InFlightTaskCancellation> for bool {
    /// Converts an `InFlightTaskCancellation` to its boolean cancellation state.
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::InFlightTaskCancellation;
    ///
    /// let cancellation = InFlightTaskCancellation::from(true);
    /// let is_cancelled: bool = cancellation.into();
    /// assert!(is_cancelled);
    /// ```
    fn from(val: InFlightTaskCancellation) -> Self {
        val.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}
