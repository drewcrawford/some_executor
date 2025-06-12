// SPDX-License-Identifier: MIT OR Apache-2.0
/*!
Task-local storage for async tasks.

This module provides task-local storage similar to thread-local storage,
but scoped to async tasks instead of threads. Task-locals allow you to
store data that is unique to each async task and can be accessed from
anywhere within that task's execution.

# Overview

The module provides two types of task-local storage:

- [`LocalKey`]: A mutable task-local that can be read and modified
- [`LocalKeyImmutable`]: An immutable task-local that can only be set via scoping

Task-locals are declared using the [`task_local!`](crate::task_local!) macro:

```
use some_executor::task_local;

task_local! {
    // Mutable task-local
    static COUNTER: u32;

    // Immutable task-local
    static const CONFIG: String;
}
```

# Task-Local vs Thread-Local

Unlike thread-local storage, task-local values:
- Are scoped to async tasks, not OS threads
- Are not inherited by spawned tasks
- Must be explicitly scoped using the `scope` method
- Are cleaned up when the task completes

# Usage Patterns

## Configuration and Context

Task-locals are ideal for storing configuration or context that needs
to be available throughout a task's execution:

```
# use some_executor::task_local;
task_local! {
    static const REQUEST_ID: String;
    static const USER_ID: u64;
}

async fn process_request(request_id: String, user_id: u64) {
    REQUEST_ID.scope(request_id, async {
        USER_ID.scope(user_id, async {
            // Both REQUEST_ID and USER_ID are available
            // to all code called from here
            handle_business_logic().await;
        }).await;
    }).await;
}

async fn handle_business_logic() {
    // Can access task-locals without passing them as parameters
    REQUEST_ID.with(|id| {
        log_event("Processing", id.unwrap(), USER_ID.get());
    });
}

fn log_event(action: &str, request_id: &str, user_id: u64) {
    println!("[{}] {} for user {}", request_id, action, user_id);
}
```

## Mutable State

Mutable task-locals can maintain state throughout a task's execution:

```
# use some_executor::task_local;
task_local! {
    static EVENTS: Vec<String>;
}

async fn track_events() {
    EVENTS.scope(Vec::new(), async {
        record_event("started");
        process_data().await;
        record_event("completed");

        // Print all events at the end
        EVENTS.with(|events| {
            for event in events.unwrap() {
                println!("Event: {}", event);
            }
        });
    }).await;
}

fn record_event(event: &str) {
    EVENTS.with_mut(|events| {
        if let Some(vec) = events {
            vec.push(event.to_string());
        }
    });
}

async fn process_data() {
    record_event("processing data");
    // ... actual processing ...
}
```

# Safety and Best Practices

1. **Always use `with` for safe access**: The `get` method panics if the
   value is not set. Use `with` to handle the `None` case gracefully.

2. **Scope values appropriately**: Task-locals should be scoped at the
   appropriate level to avoid unnecessary overhead and ensure cleanup.

3. **Don't rely on inheritance**: Task-locals are not inherited by
   spawned tasks. Each task starts with no task-locals set.

4. **Consider immutability**: Use `const` task-locals for values that
   shouldn't change during execution, like configuration or IDs.
*/

use std::cell::RefCell;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Declares task-local storage keys.
///
/// This macro is similar to `thread_local!` but creates storage that is
/// scoped to async tasks rather than OS threads. Task-locals are useful
/// for storing context that needs to be available throughout a task's
/// execution without explicitly passing it through function parameters.
///
/// # Syntax
///
/// The macro supports two types of task-locals:
///
/// - **Mutable**: `static NAME: TYPE` - Can be read and modified
/// - **Immutable**: `static const NAME: TYPE` - Can only be set via scoping
///
/// Multiple task-locals can be declared in a single macro invocation.
///
/// # Examples
///
/// ## Basic Usage
///
/// ```
/// use some_executor::task_local;
///
/// task_local! {
///     static COUNTER: u32;
///     static const USER_ID: u64;
/// }
///
/// async fn example() {
///     // Mutable task-local
///     COUNTER.scope(0, async {
///         COUNTER.set(1);
///         assert_eq!(COUNTER.get(), 1);
///         COUNTER.replace(2);
///         assert_eq!(COUNTER.get(), 2);
///     }).await;
///     
///     // Immutable task-local
///     USER_ID.scope(12345, async {
///         assert_eq!(USER_ID.get(), 12345);
///         // USER_ID.set(99999); // This would not compile!
///     }).await;
/// }
/// ```
///
/// ## Multiple Declarations
///
/// ```
/// # use some_executor::task_local;
/// task_local! {
///     static REQUEST_ID: String;
///     static TRACE_ID: String;
///     static const ENVIRONMENT: String;
///     static const REGION: String;
/// }
/// ```
///
/// ## With Attributes
///
/// ```
/// # use some_executor::task_local;
/// task_local! {
///     /// The current user's session ID
///     #[allow(dead_code)]
///     pub static SESSION_ID: String;
///     
///     /// Application configuration
///     pub static const CONFIG: String;
/// }
/// ```
///
/// # Differences from `thread_local!`
///
/// 1. **Scoping**: Task-locals must be explicitly scoped using the `scope` method
/// 2. **Inheritance**: Task-locals are not inherited by spawned tasks
/// 3. **Lifetime**: Values are automatically cleaned up when the scope ends
/// 4. **Access**: Values may not be set outside of a scope
///
/// # Generated Types
///
/// - Mutable task-locals generate a [`LocalKey<T>`]
/// - Immutable task-locals generate a [`LocalKeyImmutable<T>`]
#[macro_export]
macro_rules! task_local {
// empty (base case for the recursion)
    () => {};

    ($(#[$attr:meta])* $vis:vis static const $name:ident: $t:ty; $($rest:tt)*) => (
        $crate::__task_local_inner_immutable!($(#[$attr])* $vis $name, $t);
        $crate::task_local!($($rest)*);
    );

    ($(#[$attr:meta])* $vis:vis static const $name:ident: $t:ty) => (
        $crate::__task_local_inner_immutable!($(#[$attr])* $vis $name, $t);
    );

    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty; $($rest:tt)*) => (
        $crate::__task_local_inner!($(#[$attr])* $vis $name, $t);
        $crate::task_local!($($rest)*);
    );

    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty) => (
        $crate::__task_local_inner!($(#[$attr])* $vis $name, $t);
    );

}

#[doc(hidden)]
#[macro_export]
macro_rules! __task_local_inner {
    ($(#[$attr:meta])* $vis:vis $name:ident, $t:ty) => {
        $(#[$attr])*
        $vis static $name: $crate::context::LocalKey<$t> = {
            std::thread_local! {
                static __KEY: std::cell::RefCell<Option<$t>> = const { std::cell::RefCell::new(None) };
            }

            $crate::context::LocalKey::new(__KEY)
        };
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __task_local_inner_immutable {
    ($(#[$attr:meta])* $vis:vis $name:ident, $t:ty) => {
        $(#[$attr])*
        $vis static $name: $crate::context::LocalKeyImmutable<$t> = {
            std::thread_local! {
                static __KEY: std::cell::RefCell<Option<$t>> = const { std::cell::RefCell::new(None) };
            }

            $crate::context::LocalKeyImmutable::new(__KEY)
        };
    };
}

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
pub struct LocalKey<T: 'static>(std::thread::LocalKey<RefCell<Option<T>>>);

impl<T: 'static> LocalKey<T> {
    #[doc(hidden)]
    pub const fn new(key: std::thread::LocalKey<RefCell<Option<T>>>) -> Self {
        LocalKey(key)
    }
}

/**
Defines an immutable task-local value.

# A word on immutability

Immutable task-locals cannot be mutated while they are in scope.  However, the meaning of this is nonintuitive.

Reading the value of the task-local (such as with [LocalKeyImmutable::get] or [LocalKeyImmutable::with]) can return different results
at different times, which may be surprising for a 'constant' value.  This is because the value is only constant for the duration of the scope,
while a future may execute across various scopes at various times.

The primary utility of this type is to forbid "downstream" modifications of the task-local.  For example,
if a task-local is set in a parent task, it may be desirable to prevent a child task from modifying it.
*/
#[derive(Debug)]
pub struct LocalKeyImmutable<T: 'static>(std::thread::LocalKey<RefCell<Option<T>>>);

impl<T: 'static> LocalKeyImmutable<T> {
    #[doc(hidden)]
    pub const fn new(key: std::thread::LocalKey<RefCell<Option<T>>>) -> Self {
        LocalKeyImmutable(key)
    }
}

/// A future that sets a value `T` of a task local for the future `F` during
/// its execution.
///
/// The value of the task-local must be `'static` and will be dropped on the
/// completion of the future.
///
/// Created by the function [`LocalKey::scope`](self::LocalKey::scope).
#[derive(Debug)]
pub(crate) struct TaskLocalFuture<V: 'static, F> {
    slot: Option<V>,
    local_key: &'static LocalKey<V>,
    future: F,
}

#[derive(Debug)]
pub(crate) struct TaskLocalImmutableFuture<V: 'static, F> {
    slot: Option<V>,
    local_key: &'static LocalKeyImmutable<V>,
    future: F,
}

impl<V, F> TaskLocalFuture<V, F> {
    //     /**
    //     Gets access to the underlying value.
    // */
    //
    //     pub(crate) fn get_val<R>(&self, closure:impl FnOnce(&V) -> R) -> R  {
    //         match self.slot {
    //             Some(ref value) => closure(value),
    //             None => self.local_key.with(|value| {
    //                 closure(value.expect("Value neither in slot nor in thread-local"))
    //             })
    //         }
    //     }
    //     /**
    //     Gets mutable access to the underlying value.
    // */
    //     pub(crate) fn get_val_mut<R>(&mut self, closure:impl FnOnce(&mut V) -> R) -> R  {
    //         match self.slot {
    //             Some(ref mut value) => closure(value),
    //             None => self.local_key.with_mut(|value| {
    //                 closure(value.expect("Value neither in slot nor in thread-local"))
    //             })
    //         }
    //     }

    // pub(crate) fn get_future(&self) -> &F {
    //     &self.future
    // }
    //
    // pub (crate) fn get_future_mut(&mut self) -> &mut F {
    //     &mut self.future
    // }
    //
    // pub(crate) fn into_future(self) -> F {
    //     self.future
    // }
}

impl<V, F> TaskLocalImmutableFuture<V, F> {
    // /**
    // Gets access to the underlying value.
    // */
    //
    // pub(crate) fn get_val<R>(&self, closure:impl FnOnce(&V) -> R) -> R  {
    //     match self.slot {
    //         Some(ref value) => closure(value),
    //         None => self.local_key.with(|value| {
    //             closure(value.expect("Value neither in slot nor in thread-local"))
    //         })
    //     }
    // }

    // pub(crate) fn get_future(&self) -> &F {
    //     &self.future
    // }

    // pub (crate) fn get_future_mut(&mut self) -> &mut F {
    //     &mut self.future
    // }

    // pub(crate) fn into_future(self) -> F {
    //     self.future
    // }
}

impl<V, F> Future for TaskLocalFuture<V, F>
where
    V: Unpin,
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        //destructure
        let (future, slot, local_key) = unsafe {
            let this = self.get_unchecked_mut();
            let future = Pin::new_unchecked(&mut this.future);
            let slot = Pin::new_unchecked(&mut this.slot);
            let local_key = Pin::new_unchecked(&mut this.local_key);
            (future, slot, local_key)
        };

        let mut_slot = Pin::get_mut(slot);
        let value = mut_slot.take().expect("No value in slot");
        let old_value = local_key.0.replace(Some(value));
        assert!(old_value.is_none(), "Task-local already set");
        let r = future.poll(cx);
        //put value back in slot
        let value = local_key.0.replace(None).expect("No value in slot");
        mut_slot.replace(value);
        r
    }
}

impl<V, F> Future for TaskLocalImmutableFuture<V, F>
where
    V: Unpin,
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        //destructure
        let (future, slot, local_key) = unsafe {
            let this = self.get_unchecked_mut();
            let future = Pin::new_unchecked(&mut this.future);
            let slot = Pin::new_unchecked(&mut this.slot);
            let local_key = Pin::new_unchecked(&mut this.local_key);
            (future, slot, local_key)
        };

        let mut_slot = Pin::get_mut(slot);
        let value = mut_slot.take().expect("No value in slot");
        let old_value = local_key.0.replace(Some(value));
        assert!(old_value.is_none(), "Task-local already set");
        let r = future.poll(cx);
        //put value back in slot
        let value = local_key.0.replace(None).expect("No value in slot");
        mut_slot.replace(value);
        r
    }
}

impl<T: 'static> LocalKey<T> {
    /// Sets the value of the task-local for the duration of the future `F`.
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

impl<T: 'static> LocalKeyImmutable<T> {
    /// Sets the value of the task-local for the duration of the future `F`.
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
    /// Unlike [`LocalKey::scope`], the value cannot be modified once set
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

    /**
    Accesses the underlying task-local inside the closure, mutably.

    # Safety
    This is unsafe because the type guarantees the type is immutable, but you are mutating it
    outside of the scope of the task-local.
    */
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
/*
boilerplates
LocalKey - underlying doesn't support clone.  This eliminates copy,eq,ord,default,etc.
from/into does not make a lot of sense, neither does asref/deref
Looks like send/sync/unpin ought to carry through
drop is sort of specious for static types
 */

#[cfg(test)]
mod tests {
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn local() {
        task_local! {
            #[allow(unused)]
            static FOO: u32;
            #[allow(unused)]
            static const BAR: u32;
        }
    }
}
