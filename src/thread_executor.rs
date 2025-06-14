// SPDX-License-Identifier: MIT OR Apache-2.0

//! Thread-local storage for executors.
//!
//! This module provides a mechanism to associate executors with specific threads,
//! allowing async code to spawn tasks without explicitly passing executor references.
//! Thread-local executors serve as a fallback in the executor discovery hierarchy,
//! sitting between task-specific executors and the global executor.
//!
//! # Overview
//!
//! The module supports two types of thread-local executors:
//!
//! - **Regular executors** (`DynExecutor`): Can spawn `Send` futures and are typically
//!   used when the executor can move work between threads.
//! - **Local executors** (`SomeLocalExecutor`): Can spawn `!Send` futures and execute
//!   them on the current thread only.
//!
//! # Usage in Executor Discovery
//!
//! Thread-local executors are part of the executor discovery hierarchy used by
//! [`current_executor`](crate::current_executor::current_executor):
//!
//! 1. Task executor (highest priority)
//! 2. **Thread executor** (this module)
//! 3. Global executor
//! 4. Last resort executor (lowest priority)
//!
//! # Examples
//!
//! ## Setting a thread executor
//!
//! ```
//! use some_executor::thread_executor::{set_thread_executor, thread_executor};
//!
//! # fn example() {
//! # let my_executor: Box<some_executor::DynExecutor> = todo!();
//! // Set an executor for the current thread
//! set_thread_executor(my_executor);
//!
//! // Later, access the thread executor
//! thread_executor(|executor| {
//!     if let Some(exec) = executor {
//!         println!("Thread has an executor");
//!     }
//! });
//! # }
//! ```
//!
//! ## Using thread-local executors with current_executor
//!
//! ```
//! use some_executor::thread_executor::set_thread_executor;
//! use some_executor::current_executor::current_executor;
//! use some_executor::SomeExecutor;
//! use some_executor::task::{Task, Configuration};
//! use some_executor::observer::Observer;
//!
//! # async fn example() {
//! # let my_executor: Box<some_executor::DynExecutor> = todo!();
//! // Set a thread-local executor
//! set_thread_executor(my_executor);
//!
//! // current_executor will find and use the thread-local executor
//! let mut executor = current_executor();
//!
//! let task = Task::without_notifications(
//!     "example".to_string(),
//!     Configuration::default(),
//!     async { println!("Running on thread executor"); },
//! );
//!
//! executor.spawn(task).detach();
//! # }
//! ```
//!
//! # Thread Safety
//!
//! Thread-local storage is inherently thread-safe as each thread has its own
//! independent storage. However, the executors themselves must be `Send + Sync`
//! for regular executors, as they may be cloned and shared across async contexts.

use crate::observer::ExecutorNotified;
use crate::{DynExecutor, SomeLocalExecutor};
use std::cell::RefCell;

// Type alias for complex types to satisfy clippy::type_complexity warnings

/// Type alias for a thread-local executor that can handle local tasks
type ThreadLocalExecutor = RefCell<
    Option<Box<dyn SomeLocalExecutor<'static, ExecutorNotifier = Box<dyn ExecutorNotified>>>>,
>;

thread_local! {
    static THREAD_EXECUTOR: RefCell<Option<Box<DynExecutor>>> = RefCell::new(None);
    static THREAD_LOCAL_EXECUTOR: ThreadLocalExecutor = RefCell::new(None);
}

/// Accesses the executor that is available for the current thread.
///
/// This function provides safe access to the thread-local executor, if one has been set.
/// The executor is accessed through a closure to ensure proper borrowing semantics.
///
/// # Parameters
///
/// - `c`: A closure that receives an optional reference to the thread's executor.
///   The closure should return a value of type `R`.
///
/// # Returns
///
/// Returns whatever value the closure produces.
///
/// # Examples
///
/// ```
/// use some_executor::thread_executor::{thread_executor, set_thread_executor};
///
/// # fn example() {
/// // Check if thread has an executor
/// let has_executor = thread_executor(|exec| exec.is_some());
/// println!("Thread has executor: {}", has_executor);
///
/// # let my_executor: Box<some_executor::DynExecutor> = todo!();
/// // Set an executor
/// set_thread_executor(my_executor);
///
/// // Clone the executor for use elsewhere
/// let cloned = thread_executor(|exec| {
///     exec.map(|e| e.clone_box())
/// });
/// # }
/// ```
pub fn thread_executor<R>(c: impl FnOnce(Option<&DynExecutor>) -> R) -> R {
    THREAD_EXECUTOR.with(|e| c(e.borrow().as_ref().map(|e| &**e)))
}

/// Sets the executor for the current thread.
///
/// This function associates an executor with the current thread. Once set, this
/// executor will be available to async code running on this thread through
/// [`thread_executor`] or [`current_executor`](crate::current_executor::current_executor).
///
/// # Parameters
///
/// - `runtime`: A boxed executor that implements the `SomeExecutor` trait.
///   This executor will be stored in thread-local storage.
///
/// # Note
///
/// Setting a new executor will replace any previously set executor for this thread.
/// There is no way to "unset" an executor once set; you can only replace it with
/// a different one.
///
/// # Examples
///
/// ```
/// use some_executor::thread_executor::{set_thread_executor, thread_executor};
///
/// # fn example() {
/// # let executor: Box<some_executor::DynExecutor> = todo!();
/// // Set a thread-local executor
/// set_thread_executor(executor);
///
/// // Verify it was set
/// thread_executor(|exec| {
///     assert!(exec.is_some());
/// });
/// # }
/// ```
///
/// ## Use with async runtimes
///
/// ```
/// use some_executor::thread_executor::set_thread_executor;
///
/// # async fn example() {
/// # let runtime_executor: Box<some_executor::DynExecutor> = todo!();
/// // Set up a thread-local executor when initializing a worker thread
/// std::thread::spawn(move || {
///     set_thread_executor(runtime_executor);
///
///     // Now any async code on this thread can access the executor
///     // through current_executor() or thread_executor()
/// });
/// # }
/// ```
pub fn set_thread_executor(runtime: Box<DynExecutor>) {
    THREAD_EXECUTOR.with(|e| {
        *e.borrow_mut() = Some(runtime);
    });
}

/// Accesses the local executor that is available for the current thread.
///
/// This function provides safe access to the thread-local executor for `!Send` futures.
/// Local executors can only execute futures on the current thread and are useful for
/// working with thread-local data or resources that cannot be safely moved between threads.
///
/// # Parameters
///
/// - `c`: A closure that receives an optional reference to the thread's local executor.
///   The executor's notifier type is erased to `Box<dyn ExecutorNotified>`.
///
/// # Returns
///
/// Returns whatever value the closure produces.
///
/// # Examples
///
/// ```
/// use some_executor::thread_executor::{thread_local_executor, set_thread_local_executor};
///
/// # fn example() {
/// // Check if thread has a local executor
/// let has_local = thread_local_executor(|exec| exec.is_some());
///
/// if !has_local {
///     println!("No local executor set for this thread");
/// }
/// # }
/// ```
///
/// ## Spawning !Send futures
///
/// ```
/// use some_executor::thread_executor::{thread_local_executor, set_thread_local_executor};
/// use some_executor::SomeLocalExecutor;
/// use some_executor::task::{Task, Configuration};
/// use std::rc::Rc;
/// # use some_executor::observer::Observer;
///
/// # fn example() {
/// # let local_exec: Box<dyn some_executor::SomeLocalExecutor<'static, ExecutorNotifier = Box<dyn some_executor::observer::ExecutorNotified>>> = todo!();
/// // First set a local executor
/// set_thread_local_executor(local_exec);
///
/// // Rc is !Send
/// let shared_data = Rc::new(42);
/// let data_clone = shared_data.clone();
///
/// // Use the thread local executor - note it needs to be mutable
/// # /*
/// thread_local_executor(|exec| {
///     if let Some(executor) = exec {
///         // In practice, you'd need a way to get mutable access
///         // This is a limitation of the current API design
///     }
/// });
/// # */
/// # }
/// ```
pub fn thread_local_executor<R>(
    c: impl FnOnce(Option<&dyn SomeLocalExecutor<ExecutorNotifier = Box<dyn ExecutorNotified>>>) -> R,
) -> R {
    THREAD_LOCAL_EXECUTOR.with(|e| c(e.borrow().as_ref().map(|e| &**e)))
}

/// Sets the local executor for the current thread.
///
/// This function associates a local executor with the current thread. Local executors
/// can spawn and execute `!Send` futures, making them suitable for working with
/// thread-local resources.
///
/// # Parameters
///
/// - `runtime`: A boxed local executor with a 'static lifetime and type-erased notifier.
///   The executor must be able to outlive any futures it spawns.
///
/// # Examples
///
/// ```
/// use some_executor::thread_executor::set_thread_local_executor;
///
/// # fn example() {
/// # let my_local_executor: Box<dyn some_executor::SomeLocalExecutor<'static, ExecutorNotifier = Box<dyn some_executor::observer::ExecutorNotified>>> = todo!();
/// // Set a local executor for the current thread
/// set_thread_local_executor(my_local_executor);
///
/// // Now !Send futures can be spawned on this thread
/// # }
/// ```
///
/// # Lifetime Requirements
///
/// The executor must have a `'static` lifetime, which means it cannot borrow from
/// stack data. This is necessary because the executor may outlive the current
/// stack frame. See the [module documentation](crate::SomeLocalExecutor) for more
/// details on lifetime requirements for local executors.
pub fn set_thread_local_executor(
    runtime: Box<dyn SomeLocalExecutor<'static, ExecutorNotifier = Box<dyn ExecutorNotified>>>,
) {
    THREAD_LOCAL_EXECUTOR.with(|e| {
        *e.borrow_mut() = Some(runtime);
    });
}
/// Sets the local executor for the current thread with automatic notifier adaptation.
///
/// This is a convenience function that wraps the provided executor in an adapter
/// that erases the specific notifier type to `Box<dyn ExecutorNotified>`. This
/// allows you to use local executors with different notifier types without manually
/// performing the type erasure.
///
/// # Parameters
///
/// - `runtime`: A local executor that implements `SomeLocalExecutor<'static>`. The
///   executor's specific notifier type will be erased.
///
/// # Type Parameters
///
/// - `E`: The concrete type of the local executor. Must implement `SomeLocalExecutor<'static>`
///   and have a `'static` lifetime.
///
/// # Examples
///
/// ```no_run
/// use some_executor::observer::TypedObserver;
/// use some_executor::thread_executor::set_thread_local_executor_adapting_notifier;
///
/// # struct MyLocalExecutor;
/// # impl some_executor::SomeLocalExecutor<'static> for MyLocalExecutor {
/// #     type ExecutorNotifier = MyNotifier;
/// #     fn spawn_local<F, N>(&mut self, _: some_executor::task::Task<F, N>) -> impl some_executor::observer::Observer<Value = F::Output>
/// #     where F: std::future::Future + 'static, N: some_executor::observer::ObserverNotified<F::Output>, F::Output: Unpin + 'static
/// #     { todo!() as TypedObserver<F::Output,MyNotifier>}
/// #     fn spawn_local_async<F, N>(&mut self, _: some_executor::task::Task<F, N>) -> impl std::future::Future<Output = impl some_executor::observer::Observer<Value = F::Output>>
/// #     where F: std::future::Future + 'static, N: some_executor::observer::ObserverNotified<F::Output>, F::Output: Unpin + 'static
/// #     { async { todo!() as TypedObserver<F::Output,MyNotifier> } }
/// #     fn spawn_local_objsafe(&mut self, _: some_executor::task::Task<std::pin::Pin<Box<dyn std::future::Future<Output = Box<dyn std::any::Any>>>>, Box<dyn some_executor::observer::ObserverNotified<(dyn std::any::Any + 'static)>>>) -> Box<dyn some_executor::observer::Observer<Value = Box<dyn std::any::Any>, Output = some_executor::observer::FinishedObservation<Box<dyn std::any::Any>>>>
/// #     { todo!() }
/// #     fn spawn_local_objsafe_async<'s>(&'s mut self, _: some_executor::task::Task<std::pin::Pin<Box<dyn std::future::Future<Output = Box<dyn std::any::Any>>>>, Box<dyn some_executor::observer::ObserverNotified<(dyn std::any::Any + 'static)>>>) -> Box<dyn std::future::Future<Output = Box<dyn some_executor::observer::Observer<Value = Box<dyn std::any::Any>, Output = some_executor::observer::FinishedObservation<Box<dyn std::any::Any>>>>> + 's>
/// #     { todo!() }
/// #     fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> { None }
/// # }
/// # struct MyNotifier;
/// # impl some_executor::observer::ExecutorNotified for MyNotifier { fn request_cancel(&mut self) {} }
///
/// // Custom local executor with its own notifier type
/// let my_executor: MyLocalExecutor = todo!();
///
/// // This function handles the notifier type erasure automatically
/// set_thread_local_executor_adapting_notifier(my_executor);
///
/// // The executor is now available with a type-erased notifier
/// ```
///
/// # Implementation Note
///
/// This function uses an internal adapter type to erase the executor's specific
/// notifier type. This allows for a uniform interface while preserving the
/// executor's functionality.
pub fn set_thread_local_executor_adapting_notifier<E: SomeLocalExecutor<'static> + 'static>(
    runtime: E,
) {
    let adapter = crate::local::OwnedSomeLocalExecutorErasingNotifier::new(runtime);
    set_thread_local_executor(Box::new(adapter));
}
