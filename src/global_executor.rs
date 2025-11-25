// SPDX-License-Identifier: MIT OR Apache-2.0

//! Global executor management for program-wide task execution.
//!
//! This module provides functionality for setting and accessing a global executor
//! that persists for the entire lifetime of the program. This is useful when you
//! need to spawn tasks from contexts where you cannot easily pass an executor
//! as a parameter, such as signal handlers or global initialization code.
//!
//! # Overview
//!
//! The global executor pattern allows you to:
//! - Set a single executor for the entire program using [`set_global_executor`]
//! - Access this executor from anywhere using [`global_executor`]
//! - Spawn tasks without needing to thread an executor through your call stack
//!
//! # Important Considerations
//!
//! - The global executor can only be set once. Attempting to set it multiple times
//!   will panic.
//! - You must initialize the global executor before attempting to use it. Accessing
//!   an uninitialized global executor will return `None`.
//! - For most use cases, [`crate::current_executor::current_executor`] is preferred
//!   as it provides more flexibility and context-aware executor selection.
//!
//! # Example Usage
//!
//! ```no_run
//! # // not runnable because we use `todo!()`
//! use some_executor::global_executor::{set_global_executor, global_executor};
//! use some_executor::DynExecutor;
//!
//! // Initialize the global executor early in your program
//! let executor: Box<DynExecutor> = todo!(); // Your executor implementation
//! set_global_executor(executor);
//!
//! // Later, from anywhere in your program
//! global_executor(|e| {
//!     if let Some(executor) = e {
//!         // Use the executor
//!         let mut executor = executor.clone_box();
//!         // spawn tasks...
//!     }
//! });
//! ```

use crate::DynExecutor;
use std::sync::OnceLock;

static GLOBAL_RUNTIME: OnceLock<Box<DynExecutor>> = OnceLock::new();

/// Accesses the global executor through a callback function.
///
/// This function provides safe access to the global executor (if one has been set)
/// through a callback pattern. The callback receives an `Option<&DynExecutor>` which
/// will be `Some` if a global executor has been initialized, or `None` otherwise.
///
/// # Arguments
///
/// * `c` - A closure that receives `Option<&DynExecutor>` and returns a value of type `R`
///
/// # Returns
///
/// Whatever value the callback function returns.
///
/// # Usage Patterns
///
/// ## Cloning the executor for task spawning
///
/// ```no_run
/// # // not runnable because there is no global executor set in this config
/// use some_executor::global_executor::global_executor;
/// use some_executor::task::{Task, ConfigurationBuilder};
///
/// let mut executor = global_executor(|e| {
///     e.expect("Global executor not initialized").clone_box()
/// });
///
/// // Now you can spawn tasks
/// let task = Task::without_notifications(
///     "my-task".to_string(),
///     ConfigurationBuilder::new().build(),
///     async { 42 },
/// );
/// # // executor.spawn(task);
/// ```
///
/// ## Checking if executor is available
///
/// ```
/// use some_executor::global_executor::global_executor;
///
/// let is_available = global_executor(|e| e.is_some());
/// if !is_available {
///     println!("Global executor not initialized");
/// }
/// ```
///
/// ## Spawning with fallback
///
/// ```
/// use some_executor::global_executor::global_executor;
/// use some_executor::current_executor::current_executor;
///
/// // Try global executor, fall back to current executor
/// let mut executor = global_executor(|e| {
///     e.map(|exec| exec.clone_box())
/// }).unwrap_or_else(|| current_executor());
/// ```
///
/// # Important Notes
///
/// - The global executor must be initialized with [`set_global_executor`] before use
/// - For most use cases, prefer [`crate::current_executor::current_executor`] which
///   provides more flexible executor discovery
/// - The callback pattern ensures thread-safe access to the global executor
pub fn global_executor<R>(c: impl FnOnce(Option<&DynExecutor>) -> R) -> R {
    let e = GLOBAL_RUNTIME.get();
    c(e.map(|e| &**e))
}

/// Sets the global executor for the entire program.
///
/// This function initializes the global executor that will be available throughout
/// the program's lifetime. Once set, the executor can be accessed from anywhere
/// using [`global_executor`].
///
/// # Arguments
///
/// * `runtime` - A boxed [`DynExecutor`] that will serve as the global executor
///
/// # Panics
///
/// This function will panic if called more than once. The global executor can only
/// be initialized once during the program's lifetime.
///
/// # Example
///
/// ```
/// use some_executor::global_executor::set_global_executor;
/// use some_executor::DynExecutor;
///
/// // Early in your program initialization
/// fn init_executor() {
///     let executor: Box<DynExecutor> = todo!(); // Your executor implementation
///     set_global_executor(executor);
/// }
/// ```
///
/// # Best Practices
///
/// - Call this function early in your program's initialization, ideally in `main()`
///   or during startup
/// - Ensure the executor is fully configured before setting it as the global executor
/// - Consider whether you actually need a global executor - in many cases,
///   [`crate::current_executor`] provides a more flexible solution
///
/// # Thread Safety
///
/// This function is thread-safe and uses internal synchronization. However, it should
/// typically be called from a single initialization point to avoid race conditions
/// where multiple threads attempt to set the global executor.
pub fn set_global_executor(runtime: Box<DynExecutor>) {
    GLOBAL_RUNTIME
        .set(runtime)
        .expect("Global runtime already set");
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::global_executor::global_executor;
    use crate::task::{ConfigurationBuilder, Task};
    use std::any::Any;

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn global_pattern() {
        #[allow(unused)]
        fn dont_execute_just_compile() {
            let mut runtime = global_executor(|e| e.unwrap().clone_box());
            let configuration = ConfigurationBuilder::new().build();
            //get a Box<dyn Any>

            let task = Task::new_objsafe(
                "test".into(),
                Box::new(async {
                    Box::new(()) as Box<dyn Any + Send + 'static>
                    // todo!()
                }),
                configuration,
                None,
            );
            runtime.spawn_objsafe(task);
        }
    }
}
