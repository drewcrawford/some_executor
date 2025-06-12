//SPDX-License-Identifier: MIT OR Apache-2.0

//! Provides access to the current executor in various contexts.
//!
//! This module implements the executor discovery mechanism that allows async code
//! to spawn tasks without explicitly passing an executor reference.

use crate::DynExecutor;

/// Retrieves the executor associated with the current task, if any.
///
/// This function checks if the current code is running within a task context
/// and whether that task has an associated executor.
///
/// # Returns
///
/// - `Some(executor)` if running within a task that has an associated executor
/// - `None` if not running within a task, or if the task has no associated executor
///
/// # Implementation Details
///
/// This accesses the task-local `TASK_EXECUTOR` variable which is set when a task
/// is polled with an executor context.
fn current_task_executor() -> Option<Box<DynExecutor>> {
    crate::task::TASK_EXECUTOR.with(|e| {
        match e {
            Some(e) => {
                //in task, but we may or may not have executor
                match e {
                    Some(e) => Some(e.clone_box()),
                    None => None,
                }
            }
            None => None, //not in a task
        }
    })
}

/// Accesses the current executor using a hierarchical discovery mechanism.
///
/// This function provides a convenient way to obtain an executor without explicitly
/// passing one through function parameters. It searches for an executor in the
/// following order:
///
/// 1. **Task executor**: If the current code is running within a task that has
///    an associated executor, that executor is returned.
/// 2. **Thread executor**: If the current thread has a thread-local executor set,
///    that executor is returned.
/// 3. **Global executor**: If a global executor has been configured for the
///    application, that executor is returned.
/// 4. **Last resort executor**: A built-in fallback executor is returned. This
///    executor provides basic functionality but may not be optimal for production use.
///
/// # Returns
///
/// Always returns a boxed executor. The function is guaranteed to return an executor,
/// falling back to the last resort executor if no other executor is available.
///
/// # Examples
///
/// ```
/// use some_executor::current_executor::current_executor;
/// use some_executor::SomeExecutor;
/// use some_executor::task::{Task, Configuration};
/// use crate::some_executor::observer::Observer;
/// 
/// # async fn example() {
/// // Get the current executor
/// let mut executor = current_executor();
/// 
/// // Use it to spawn a task
/// let task = Task::without_notifications(
///     "example task".to_string(),
///     async {
///         println!("Hello from spawned task");
///     },
///     Configuration::default()
/// );
/// 
/// let _observer = executor.spawn(task).detach();
/// # }
/// ```
///
/// # Performance Considerations
///
/// Each call to this function performs the full discovery process. If you need
/// to spawn multiple tasks, consider storing the executor in a variable rather
/// than calling this function repeatedly.
///
/// ```
/// use some_executor::current_executor::current_executor;
/// use some_executor::SomeExecutor;
/// use some_executor::task::{Task, Configuration};
/// use crate::some_executor::observer::Observer;
///
/// # async fn spawn_many_tasks() {
/// // Good: Store the executor
/// let mut executor = current_executor();
/// for i in 0..100 {
///     let task = Task::without_notifications(
///         format!("task-{}", i),
///         async move {
///             println!("Task {}", i);
///         },
///         Configuration::default()
///     );
///     executor.spawn(task);
/// }
/// 
/// // Less efficient: Repeated discovery
/// for i in 0..100 {
///     let mut executor = current_executor(); // Performs discovery each time
///     let task = Task::without_notifications(
///         format!("task-{}", i),
///         async move {
///             println!("Task {}", i);
///         },
///         Configuration::default()
///     );
///     executor.spawn(task).detach();
/// }
/// # }
/// ```
///
/// # Last Resort Executor
///
/// The last resort executor is a simple, built-in executor that ensures async code
/// can always make progress. However, it may not be optimal for production use as
/// it lacks advanced features like work stealing, priority scheduling, or
/// sophisticated thread management. It's recommended to configure a proper executor
/// for production applications.
pub fn current_executor() -> Box<DynExecutor> {
    if let Some(executor) = current_task_executor() {
        executor
    } else if let Some(executor) =
        crate::thread_executor::thread_executor(|e| e.map(|e| e.clone_box()))
    {
        executor
    } else if let Some(executor) =
        crate::global_executor::global_executor(|e| e.map(|e| e.clone_box()))
    {
        executor
    } else {
        Box::new(crate::last_resort::LastResortExecutor::new())
    }
}
