// SPDX-License-Identifier: MIT OR Apache-2.0

//! Mutable task-local storage implementation.

use std::cell::RefCell;
use std::future::Future;

use crate::context::futures::TaskLocalFuture;

/// A task-local storage key for mutable values.
///
/// This type allows you to store data that is unique to each async task,
/// similar to thread-local storage but scoped to async tasks instead of threads.
///
/// # Examples
///
/// ```
/// some_executor::task_local! {
///     static MY_VALUE: u32;
/// }
///
/// async fn example() {
///     // Set a value for the current task
///     MY_VALUE.set(42);
///     
///     // Get the value
///     assert_eq!(MY_VALUE.get(), 42);
///     
///     // Replace the value
///     let old = MY_VALUE.replace(100);
///     assert_eq!(old, 42);
///     assert_eq!(MY_VALUE.get(), 100);
/// }
/// ```
///
/// Task-local values are not inherited by spawned tasks.
#[derive(Debug)]
pub struct LocalKey<T: 'static>(pub(crate) std::thread::LocalKey<RefCell<Option<T>>>);

impl<T: 'static> LocalKey<T> {
    /// Creates a new `LocalKey` from a thread-local storage key.
    ///
    /// This is an internal method used by the `task_local!` macro and should
    /// not be called directly by users.
    #[doc(hidden)]
    pub const fn new(key: std::thread::LocalKey<RefCell<Option<T>>>) -> Self {
        LocalKey(key)
    }

    /// Internal method to create a scoped task-local future.
    ///
    /// This method is used internally by the public `scope` method. It creates
    /// a `TaskLocalFuture` that will manage the task-local value during the
    /// execution of the provided future.
    pub(crate) fn scope_internal<F>(&'static self, value: T, f: F) -> TaskLocalFuture<T, F>
    where
        F: Future,
    {
        TaskLocalFuture {
            slot: Some(value),
            local_key: self,
            future: f,
        }
    }

    /// Sets the task-local value for the duration of a future.
    ///
    /// The value will be available to the future and any code it calls,
    /// but will be removed when the future completes.
    ///
    /// # Examples
    ///
    /// ```
    /// # use some_executor::task_local;
    /// task_local! {
    ///     static ID: u64;
    /// }
    ///
    /// async fn process() {
    ///     println!("Processing with ID: {}", ID.get());
    /// }
    ///
    /// async fn main_task() {
    ///     ID.scope(123, async {
    ///         process().await; // Will print "Processing with ID: 123"
    ///         
    ///         ID.scope(456, async {
    ///             process().await; // Will print "Processing with ID: 456"
    ///         }).await;
    ///         
    ///         process().await; // Will print "Processing with ID: 123"
    ///     }).await;
    ///     
    ///     // ID.get() would panic here because it's outside the scope
    /// }
    /// ```
    pub fn scope<F>(&'static self, value: T, f: F) -> impl Future<Output = F::Output>
    where
        F: Future,
        T: Unpin,
    {
        self.scope_internal(value, f)
    }

    /// Accesses the task-local value through a closure.
    ///
    /// This is the safest way to access a task-local value as it handles
    /// the case where the value might not be set.
    ///
    /// # Examples
    ///
    /// ```
    /// # use some_executor::task_local;
    /// task_local! {
    ///     static CONFIG: String;
    /// }
    ///
    /// async fn get_config_len() -> usize {
    ///     CONFIG.with(|config| {
    ///         config.map(|s| s.len()).unwrap_or(0)
    ///     })
    /// }
    ///
    /// async fn example() {
    ///     // Before setting, the value is None
    ///     let len = get_config_len().await;
    ///     assert_eq!(len, 0);
    ///     
    ///     CONFIG.scope("production".to_string(), async {
    ///         let len = get_config_len().await;
    ///         assert_eq!(len, 10);
    ///     }).await;
    /// }
    /// ```
    pub fn with<F, R>(&'static self, f: F) -> R
    where
        F: FnOnce(Option<&T>) -> R,
    {
        self.0.with(|slot| {
            let value = slot.borrow();
            f(value.as_ref())
        })
    }

    /// Accesses the task-local value mutably through a closure.
    ///
    /// This allows you to modify the task-local value in place.
    ///
    /// # Examples
    ///
    /// ```
    /// # use some_executor::task_local;
    /// task_local! {
    ///     static COUNTER: Vec<i32>;
    /// }
    ///
    /// async fn increment_counter() {
    ///     COUNTER.with_mut(|counter| {
    ///         if let Some(vec) = counter {
    ///             vec.push(vec.len() as i32);
    ///         }
    ///     });
    /// }
    ///
    /// async fn example() {
    ///     COUNTER.scope(vec![0], async {
    ///         increment_counter().await;
    ///         increment_counter().await;
    ///         
    ///         COUNTER.with(|counter| {
    ///             assert_eq!(counter.unwrap(), &vec![0, 1, 2]);
    ///         });
    ///     }).await;
    /// }
    /// ```
    pub fn with_mut<F, R>(&'static self, f: F) -> R
    where
        F: FnOnce(Option<&mut T>) -> R,
    {
        self.0.with(|slot| {
            let mut value = slot.borrow_mut();
            f(value.as_mut())
        })
    }

    /// Returns a copy of the task-local value.
    ///
    /// # Panics
    ///
    /// Panics if the task-local value is not set.
    ///
    /// # Examples
    ///
    /// ```
    /// # use some_executor::task_local;
    /// task_local! {
    ///     static ID: u64;
    /// }
    ///
    /// async fn get_current_id() -> u64 {
    ///     ID.get()
    /// }
    ///
    /// async fn example() {
    ///     ID.scope(42, async {
    ///         let id = get_current_id().await;
    ///         assert_eq!(id, 42);
    ///     }).await;
    /// }
    /// ```
    ///
    /// Use [`with`](Self::with) for safe access without panicking:
    ///
    /// ```
    /// # use some_executor::task_local;
    /// # task_local! { static ID: u64; }
    /// async fn get_id_or_default() -> u64 {
    ///     ID.with(|id| id.copied().unwrap_or(0))
    /// }
    /// ```
    pub fn get(&'static self) -> T
    where
        T: Copy,
    {
        self.0.with(|slot| {
            let value = slot.borrow();
            value.expect("Task-local not set")
        })
    }

    /// Sets the task-local value.
    ///
    /// This directly sets the value without running any lazy initialization.
    /// If a value was already set, it will be replaced.
    ///
    /// # Examples
    ///
    /// ```
    /// # use some_executor::task_local;
    /// task_local! {
    ///     static NAME: String;
    /// }
    ///
    /// async fn example() {
    ///     // Set initial value
    ///     NAME.set("Alice".to_string());
    ///     NAME.with(|name| {
    ///         assert_eq!(name.unwrap(), "Alice");
    ///     });
    ///     
    ///     // Replace with new value
    ///     NAME.set("Bob".to_string());
    ///     NAME.with(|name| {
    ///         assert_eq!(name.unwrap(), "Bob");
    ///     });
    /// }
    /// ```
    pub fn set(&'static self, value: T) {
        self.0.set(Some(value))
    }

    /// Replaces the task-local value, returning the old value.
    ///
    /// # Panics
    ///
    /// Panics if the task-local value is not set.
    ///
    /// # Examples
    ///
    /// ```
    /// # use some_executor::task_local;
    /// task_local! {
    ///     static GENERATION: u32;
    /// }
    ///
    /// async fn next_generation() -> u32 {
    ///     let current = GENERATION.get();
    ///     GENERATION.replace(current + 1)
    /// }
    ///
    /// async fn example() {
    ///     GENERATION.set(1);
    ///     
    ///     let old = next_generation().await;
    ///     assert_eq!(old, 1);
    ///     assert_eq!(GENERATION.get(), 2);
    ///     
    ///     let old = next_generation().await;
    ///     assert_eq!(old, 2);
    ///     assert_eq!(GENERATION.get(), 3);
    /// }
    /// ```
    pub fn replace(&'static self, value: T) -> T {
        self.0.replace(Some(value)).expect("Task-local not set")
    }
}
