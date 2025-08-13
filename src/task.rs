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
mod convenience;
mod dyn_spawned;
mod objsafe;
mod spawn;
mod spawned;
mod task_local;

// Re-exports from submodules
pub use convenience::DefaultFuture;
pub use dyn_spawned::{DynLocalSpawnedTask, DynSpawnedTask};
pub use objsafe::{
    BoxedLocalFuture, BoxedLocalObserverNotifier, BoxedSendFuture, BoxedSendObserverNotifier,
    ObjSafeLocalTask, ObjSafeStaticTask, ObjSafeTask,
};
pub use spawn::{
    SpawnLocalObjSafeResult, SpawnLocalResult, SpawnObjSafeResult, SpawnResult,
    SpawnStaticObjSafeResult, SpawnStaticResult,
};
pub use spawned::{SpawnedLocalTask, SpawnedStaticTask, SpawnedTask};
pub use task_local::{
    IS_CANCELLED, InFlightTaskCancellation, TASK_EXECUTOR, TASK_ID, TASK_LABEL,
    TASK_LOCAL_EXECUTOR, TASK_PRIORITY, TASK_STATIC_EXECUTOR,
};

use crate::Priority;
use crate::hint::Hint;
use std::convert::Infallible;
use std::fmt::Debug;
use std::future::Future;
use std::sync::atomic::AtomicU64;

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
    /// Creates a new task with optional completion notifications.
    ///
    /// This constructor creates a task that can optionally notify an observer when it completes.
    /// If you don't need notifications, consider using [`without_notifications`](Self::without_notifications) instead.
    ///
    /// # Arguments
    ///
    /// * `label` - A human-readable label for debugging and monitoring
    /// * `configuration` - Runtime hints and scheduling preferences
    /// * `notifier` - Optional observer to notify on task completion
    /// * `future` - The async computation to execute
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration};
    /// use some_executor::observer::ObserverNotified;
    ///
    /// # struct MyNotifier;
    /// # impl ObserverNotified<i32> for MyNotifier {
    /// #     fn notify(&mut self, value: &i32) {}
    /// # }
    ///
    /// let task = Task::with_notifications(
    ///     "compute-result".to_string(),
    ///     Configuration::default(),
    ///     Some(MyNotifier),
    ///     async { 42 }
    /// );
    /// ```
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
}

//infalliable notification methods

impl<F: Future> Task<F, Infallible> {
    /// Creates a task without completion notifications.
    ///
    /// This is a convenience constructor for tasks that don't need observers.
    /// It's equivalent to calling [`with_notifications`](Self::with_notifications) with `None`
    /// but avoids the need to specify the notification type parameter.
    ///
    /// # Arguments
    ///
    /// * `label` - A human-readable label for debugging and monitoring
    /// * `configuration` - Runtime hints and scheduling preferences
    /// * `future` - The async computation to execute
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration};
    ///
    /// let task = Task::without_notifications(
    ///     "background-work".to_string(),
    ///     Configuration::default(),
    ///     async {
    ///         println!("Working in the background");
    ///         42
    ///     }
    /// );
    /// ```
    pub fn without_notifications(label: String, configuration: Configuration, future: F) -> Self {
        Task::with_notifications(label, configuration, None, future)
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
    Equiv + use<E, R, F, N> + use<E, R, F, N>alent to [TaskID::to_u64].
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

        impl<'future> std::fmt::Debug for ExLocalExecutor<'future> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct("ExLocalExecutor")
                    .field("tasks", &format!("{} tasks", self.0.len()))
                    .finish()
            }
        }

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
                    Box<dyn ObserverNotified<dyn Any + 'static>>,
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
                    Box<dyn ObserverNotified<dyn Any + 'static>>,
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
    fn test_nested_spawn_local_current_no_panic() {
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
