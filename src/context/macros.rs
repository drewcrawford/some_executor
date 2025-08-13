// SPDX-License-Identifier: MIT OR Apache-2.0

//! Macros for task-local storage.

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
/// - Mutable task-locals generate a [`LocalKey<T>`](crate::context::LocalKey)
/// - Immutable task-locals generate a [`LocalKeyImmutable<T>`](crate::context::LocalKeyImmutable)
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
