// SPDX-License-Identifier: MIT OR Apache-2.0

//! Immutable task-local storage implementation.

use std::cell::RefCell;
use std::future::Future;

use crate::context::futures::TaskLocalImmutableFuture;

/// An immutable task-local storage key.
///
/// This type provides task-local storage that cannot be modified once set within
/// a scope, offering stronger guarantees than [`LocalKey`](crate::context::LocalKey) about value stability.
///
/// # Immutability Semantics
///
/// The term "immutable" here has specific semantics that may be counterintuitive:
///
/// - **Within a scope**: Once set via `scope()`, the value cannot be changed
///   by `set()` or `replace()` methods (these methods don't exist)
/// - **Across scopes**: The same task-local can have different values in nested
///   scopes, and a future may observe different values as it executes across
///   scope boundaries
/// - **Primary use case**: Preventing downstream code from modifying configuration
///   or context values that should remain stable
///
/// # Examples
///
/// ```
/// # use some_executor::task_local;
/// task_local! {
///     static const CONFIG: String;
///     static MUTABLE_STATE: Vec<String>;
/// }
///
/// async fn demonstrate_immutability() {
///     CONFIG.scope("production".to_string(), async {
///         // This would not compile - no set() method:
///         // CONFIG.set("development".to_string());
///         
///         // Access the immutable value
///         CONFIG.with(|cfg| {
///             assert_eq!(cfg.as_ref().unwrap().as_str(), "production");
///         });
///         
///         // But MUTABLE_STATE can be modified:
///         MUTABLE_STATE.scope(vec![], async {
///             MUTABLE_STATE.with_mut(|v| {
///                 if let Some(vec) = v {
///                     vec.push("log".to_string());
///                 }
///             });
///         }).await;
///     }).await;
/// }
/// ```
///
/// # When to Use
///
/// Use `LocalKeyImmutable` for:
/// - Request IDs, trace IDs, and correlation identifiers
/// - User context and authentication information  
/// - Environment configuration
/// - Any value that should remain constant within a scope
///
/// Use regular [`LocalKey`](crate::context::LocalKey) for:
/// - Accumulators and counters
/// - Mutable state that changes during execution
/// - Caches and temporary storage
#[derive(Debug)]
pub struct LocalKeyImmutable<T: 'static>(pub(crate) std::thread::LocalKey<RefCell<Option<T>>>);

impl<T: 'static> LocalKeyImmutable<T> {
    /// Creates a new `LocalKeyImmutable` from a thread-local storage key.
    ///
    /// This is an internal method used by the `task_local!` macro and should
    /// not be called directly by users.
    #[doc(hidden)]
    pub const fn new(key: std::thread::LocalKey<RefCell<Option<T>>>) -> Self {
        LocalKeyImmutable(key)
    }

    /// Internal method to create a scoped immutable task-local future.
    ///
    /// This method is used internally by the public `scope` method. It creates
    /// a `TaskLocalImmutableFuture` that will manage the immutable task-local
    /// value during the execution of the provided future.
    pub(crate) fn scope_internal<F>(&'static self, value: T, f: F) -> TaskLocalImmutableFuture<T, F>
    where
        F: Future,
    {
        TaskLocalImmutableFuture {
            slot: Some(value),
            local_key: self,
            future: f,
        }
    }

    /// Sets the immutable task-local value for the duration of a future.
    ///
    /// Unlike [`LocalKey::scope`](crate::context::LocalKey::scope), the value cannot be modified once set
    /// within the scope. This is useful for configuration values that should
    /// remain constant during execution.
    ///
    /// # Examples
    ///
    /// ```
    /// # use some_executor::task_local;
    /// task_local! {
    ///     static const ENVIRONMENT: String;
    /// }
    ///
    /// async fn log_with_env(message: &str) {
    ///     ENVIRONMENT.with(|env| {
    ///         println!("[{}] {}", env.unwrap(), message);
    ///     });
    /// }
    ///
    /// async fn main_task() {
    ///     ENVIRONMENT.scope("production".to_string(), async {
    ///         log_with_env("Starting server").await;
    ///         
    ///         // Unlike mutable task-locals, this would not compile:
    ///         // ENVIRONMENT.set("development".to_string()); // Error!
    ///         
    ///         ENVIRONMENT.scope("staging".to_string(), async {
    ///             log_with_env("In staging context").await;
    ///         }).await;
    ///         
    ///         log_with_env("Back in production").await;
    ///     }).await;
    /// }
    /// ```
    pub fn scope<F>(&'static self, value: T, f: F) -> impl Future<Output = F::Output>
    where
        F: Future,
        T: Unpin,
    {
        self.scope_internal(value, f)
    }

    /// Returns a copy of the immutable task-local value.
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
    ///     static const USER_ID: u64;
    /// }
    ///
    /// async fn get_user_permissions() -> Vec<String> {
    ///     let user_id = USER_ID.get();
    ///     // Fetch permissions for user_id...
    ///     vec![format!("read:user:{}", user_id)]
    /// }
    ///
    /// async fn example() {
    ///     USER_ID.scope(12345, async {
    ///         let perms = get_user_permissions().await;
    ///         assert_eq!(perms[0], "read:user:12345");
    ///     }).await;
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

    /// Accesses the immutable task-local value through a closure.
    ///
    /// This is the safest way to access an immutable task-local value as it
    /// handles the case where the value might not be set.
    ///
    /// # Examples
    ///
    /// ```
    /// # use some_executor::task_local;
    /// task_local! {
    ///     static const REQUEST_ID: String;
    /// }
    ///
    /// fn log_with_request_id(message: &str) {
    ///     REQUEST_ID.with(|id| {
    ///         match id {
    ///             Some(request_id) => println!("[{}] {}", request_id, message),
    ///             None => println!("[no-request-id] {}", message),
    ///         }
    ///     });
    /// }
    ///
    /// async fn handle_request() {
    ///     log_with_request_id("Processing request");
    /// }
    ///
    /// async fn example() {
    ///     // Without request ID
    ///     handle_request().await;
    ///     
    ///     // With request ID
    ///     REQUEST_ID.scope("req-123".to_string(), async {
    ///         handle_request().await;
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

    /// Accesses the underlying task-local inside the closure, mutably.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it violates the immutability guarantee
    /// of `LocalKeyImmutable`. It allows mutation of a value that is supposed
    /// to be immutable within its scope.
    ///
    /// This method is intended for internal use only and should not be exposed
    /// in the public API. It may be used by the framework itself for special
    /// cases where controlled mutation is necessary.
    ///
    /// # Panics
    ///
    /// May panic if the value is already borrowed, following `RefCell` borrowing
    /// rules.
    pub(crate) unsafe fn with_mut<F, R>(&'static self, f: F) -> R
    where
        F: FnOnce(&mut Option<T>) -> R,
    {
        self.0.with(|slot| {
            let mut value = slot.borrow_mut();
            f(&mut value)
        })
    }
}
