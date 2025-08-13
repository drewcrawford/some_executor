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
//! The module supports three types of thread-local executors:
//!
//! - **Regular executors** (`DynExecutor`): Can spawn `Send` futures and are typically
//!   used when the executor can move work between threads.
//! - **Local executors** (`SomeLocalExecutor`): Can spawn `!Send` futures and execute
//!   them on the current thread only.
//! - **Static executors** (`SomeStaticExecutor`): Can spawn `'static` futures that do not
//!   need to be `Send`, suitable for static data with thread-local execution.
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
//! ## Setting a thread-static executor
//!
//! ```
//! use some_executor::thread_executor::{set_thread_static_executor, thread_static_executor};
//!
//! # fn example() {
//! # let my_static_executor: Box<dyn some_executor::SomeStaticExecutor<ExecutorNotifier = Box<dyn some_executor::observer::ExecutorNotified>>> = todo!();
//! // Set a static executor for the current thread
//! set_thread_static_executor(my_static_executor);
//!
//! // Later, access the thread-static executor (always returns a valid executor)
//! thread_static_executor(|executor| {
//!     // executor is always valid - either user-provided or last resort
//!     println!("Thread has a static executor");
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
use crate::task::Configuration;
use crate::{DynExecutor, SomeLocalExecutor, SomeStaticExecutor};
use std::cell::RefCell;
use std::rc::Rc;
// Type alias for complex types to satisfy clippy::type_complexity warnings

/// Type alias for a thread-local executor that can handle local tasks
type ThreadLocalExecutor = RefCell<
    Option<
        Rc<
            RefCell<
                Box<dyn SomeLocalExecutor<'static, ExecutorNotifier = Box<dyn ExecutorNotified>>>,
            >,
        >,
    >,
>;

/// Type alias for a thread-static executor that can handle static tasks
type ThreadStaticExecutor =
    RefCell<Option<Box<dyn SomeStaticExecutor<ExecutorNotifier = Box<dyn ExecutorNotified>>>>>;

thread_local! {
    static THREAD_EXECUTOR: RefCell<Option<Box<DynExecutor>>> = RefCell::new(None);
    static THREAD_LOCAL_EXECUTOR: ThreadLocalExecutor = RefCell::new(None);
    static THREAD_STATIC_EXECUTOR: ThreadStaticExecutor = RefCell::new(None);
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
/// If no executor has been set for the thread, the function will panic.
/// You must configure a local executor before spawning local tasks.
///
/// # Parameters
///
/// - `c`: A closure that receives a reference to the thread's local executor.
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
/// thread_local_executor(|exec| {
///     if let Some(executor_rc) = exec {
///         //do something with the local executor
///     }
/// });
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
/// // Rc is !Send
/// let shared_data = Rc::new(42);
/// let data_clone = shared_data.clone();
///
/// // Use the thread local executor to spawn a !Send future
/// thread_local_executor(|executor_rc| {
///     if let Some(executor_rc) = executor_rc {
///         let task = Task::without_notifications(
///             "local_task".to_string(),
///             Configuration::default(),
///             async move {
///                 println!("Running !Send future with data: {:?}", data_clone);
///                 42
///             },
///         );
///         let _observer = executor_rc.borrow_mut().spawn_local_objsafe(task.into_objsafe_local());
///     }
/// });
/// # }
/// ```
pub fn thread_local_executor<R>(
    c: impl FnOnce(
        Option<
            Rc<RefCell<Box<dyn SomeLocalExecutor<ExecutorNotifier = Box<dyn ExecutorNotified>>>>>,
        >,
    ) -> R,
) -> R {
    THREAD_LOCAL_EXECUTOR.with(|e| {
        let borrowed = e.borrow();
        match borrowed.as_ref() {
            Some(executor_rc) => {
                let executor_rc = executor_rc.clone();
                drop(borrowed); // Release the borrow before calling the closure
                c(Some(executor_rc))
            }
            None => {
                drop(borrowed); // Release the borrow before calling the closure
                c(None)
            }
        }
    })
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
        let executor_rc = Rc::new(RefCell::new(runtime));
        *e.borrow_mut() = Some(executor_rc);
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
/// ```
/// use some_executor::observer::TypedObserver;
/// use some_executor::thread_executor::set_thread_local_executor_adapting_notifier;
///
/// # #[derive(Debug)]
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
/// let my_executor: MyLocalExecutor = MyLocalExecutor;
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

/// Accesses the static executor that is available for the current thread.
///
/// This function provides safe access to the thread-static executor. If a user-provided
/// executor has been set via `set_thread_static_executor`, it will be used. Otherwise,
/// the static last resort executor will be used as a fallback, ensuring that a valid
/// executor is always available.
///
/// # Parameters
///
/// - `c`: A closure that receives a reference to the thread's static executor.
///   The closure should return a value of type `R`.
///
/// # Returns
///
/// Returns whatever value the closure produces.
///
/// # Examples
///
/// ```
/// use some_executor::thread_executor::{thread_static_executor, set_thread_static_executor};
///
/// # fn example() {
/// // Always get a valid static executor (will use last resort if none set)
/// let result = thread_static_executor(|exec| {
///     // exec is always valid - either user-provided or last resort
///     exec.clone_box().executor_notifier().is_some()
/// });
/// println!("Executor notifier available: {}", result);
/// # }
/// ```
pub fn thread_static_executor<R>(
    c: impl FnOnce(&dyn SomeStaticExecutor<ExecutorNotifier = Box<dyn ExecutorNotified>>) -> R,
) -> R {
    THREAD_STATIC_EXECUTOR.with(|e| {
        let borrowed = e.borrow();
        if let Some(executor) = borrowed.as_ref() {
            c(executor.as_ref())
        } else {
            // Use the static last resort executor as fallback
            let last_resort = crate::static_last_resort::StaticLastResortExecutor::new();
            c(&last_resort)
        }
    })
}

/// Sets the static executor for the current thread.
///
/// This function associates a static executor with the current thread. Static executors
/// can spawn `'static` futures that do not need to be `Send`, making them suitable for
/// scenarios where you have static data but need thread-local execution.
///
/// # Parameters
///
/// - `runtime`: A boxed static executor with a type-erased notifier.
///   The executor will be stored in thread-local storage.
///
/// # Note
///
/// Setting a new executor will replace any previously set static executor for this thread.
/// There is no way to "unset" an executor once set; you can only replace it with
/// a different one.
///
/// # Examples
///
/// ```
/// use some_executor::thread_executor::{set_thread_static_executor, thread_static_executor};
///
/// # fn example() {
/// # let executor: Box<dyn some_executor::SomeStaticExecutor<ExecutorNotifier = Box<dyn some_executor::observer::ExecutorNotified>>> = todo!();
/// // Set a thread-static executor
/// set_thread_static_executor(executor);
///
/// // Verify it can be accessed (will always have a valid executor)
/// thread_static_executor(|exec| {
///     // exec is always valid - either user-provided or last resort
///     let _notifier = exec.clone_box().executor_notifier();
/// });
/// # }
/// ```
pub fn set_thread_static_executor(
    runtime: Box<dyn SomeStaticExecutor<ExecutorNotifier = Box<dyn ExecutorNotified>>>,
) {
    THREAD_STATIC_EXECUTOR.with(|e| {
        *e.borrow_mut() = Some(runtime);
    });
}

/// Sets the static executor for the current thread with automatic notifier adaptation.
///
/// This is a convenience function that wraps the provided executor in an adapter
/// that erases the specific notifier type to `Box<dyn ExecutorNotified>`. This
/// allows you to use static executors with different notifier types without manually
/// performing the type erasure.
///
/// # Parameters
///
/// - `runtime`: A static executor that implements `SomeStaticExecutor`. The
///   executor's specific notifier type will be erased.
///
/// # Type Parameters
///
/// - `E`: The concrete type of the static executor. Must implement `SomeStaticExecutor`
///   and have a `'static` lifetime.
///
/// # Examples
///
/// ```
/// use some_executor::thread_executor::set_thread_static_executor_adapting_notifier;
///
/// # #[derive(Debug)]
/// # struct MyStaticExecutor;
/// # struct MyNotifier;
/// # impl some_executor::observer::ExecutorNotified for MyNotifier { fn request_cancel(&mut self) {} }
/// # impl some_executor::SomeStaticExecutor for MyStaticExecutor {
/// #     type ExecutorNotifier = Box<dyn some_executor::observer::ExecutorNotified>;
/// #     fn spawn_static<F, N>(&mut self, _: some_executor::task::Task<F, N>) -> impl some_executor::observer::Observer<Value = F::Output>
/// #     where F: std::future::Future + 'static, N: some_executor::observer::ObserverNotified<F::Output>, F::Output: Unpin + 'static
/// #     { todo!() as some_executor::observer::TypedObserver<F::Output,Box<dyn some_executor::observer::ExecutorNotified>>}
/// #     fn spawn_static_async<F, N>(&mut self, _: some_executor::task::Task<F, N>) -> impl std::future::Future<Output = impl some_executor::observer::Observer<Value = F::Output>>
/// #     where F: std::future::Future + 'static, N: some_executor::observer::ObserverNotified<F::Output>, F::Output: Unpin + 'static
/// #     { async { todo!() as some_executor::observer::TypedObserver<F::Output,Box<dyn some_executor::observer::ExecutorNotified>> } }
/// #     fn spawn_static_objsafe(&mut self, _: some_executor::ObjSafeStaticTask) -> some_executor::BoxedStaticObserver { todo!() }
/// #     fn spawn_static_objsafe_async<'s>(&'s mut self, _: some_executor::ObjSafeStaticTask) -> some_executor::BoxedStaticObserverFuture<'s> { todo!() }
/// #     fn clone_box(&self) -> Box<some_executor::DynStaticExecutor> { todo!() }
/// #     fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> { None }
/// # }
/// // Use a custom static executor
/// let executor = MyStaticExecutor;
/// set_thread_static_executor_adapting_notifier(executor);
/// ```
///
/// # Implementation Note
///
/// This function uses an internal adapter type to erase the executor's specific
/// notifier type. This allows for a uniform interface while preserving the
/// executor's functionality.
pub fn set_thread_static_executor_adapting_notifier<E: SomeStaticExecutor + 'static>(runtime: E) {
    let adapter = crate::static_support::OwnedSomeStaticExecutorErasingNotifier::new(runtime);
    set_thread_static_executor(Box::new(adapter));
}

/**
Pins a task to run on the current thread; converts non-Send futures to Send futures.

# Discussion

In Rust we prefer Send futures, which allow the executor to move tasks between threads at await
points.  Doing this allows the executor to rebalance the load after futures have begun executing.
For example, the future can resume on the first available thread, rather than the thread it started
on.

This optimization requires that the future is Send, which means that it can't hold a non-Send type
across an await point.  This is a problem for futures that work with non-Send types, such as
Rc or RefCell.

When this is a problem, you can use this function to pin a non-Send task to the current thread.

# Downsides

This is a completely legitimate solution to the problem but it has some downsides:
1.  By nature, a non-Send future cannot be moved around in the thread pool, so it is necessarily
    less efficient than a Send future.
2.  There is some small runtime overhead to handing the Send-to-!Send mismatch.
3.  We go ahead and spawn the task before the return future is polled, which is nonstandard
    in Rust.  However, it is necessary because we must use the current thread to run non-Send tasks.
4.  Cancellation is not supported very well.

Because of these downsides, consider these alternatives to this function:

1.  Consider using Send/Sync types where available.
2.  Consider using a block scope to isolate non-Send types when they don't need to be held across
    await points.  See the example at https://rust-lang.github.io/async-book/07_workarounds/03_send_approximation.html.
3.  Consider using the [SomeStaticExecutor] methods directly.  The trouble is the trait itself
    does not require the observer to be Send as not all executors will support it.  But if you know
    the concrete type and it supports this, you can use it directly.

This function primarily comes into play when none of the other alternatives are viable, such as
when Send/Sync types are unavoidable, must be held across await, the executor type is either
erased or does not support Send.

# See also
[Task.pin_current] for a more idiomatic way to pin a task to the current thread.


*/
pub fn pin_static_to_thread<E: SomeStaticExecutor, R, F: Future<Output = R>, N>(
    executor: &mut E,
    task: crate::Task<F, N>,
) -> impl Future<Output = R> + Send + use<E, R, F, N>
where
    F: 'static,
    R: 'static + Send,
{
    use crate::observer::Observer;
    let (c, fut) = r#continue::continuation();
    //move task into parts
    let label = task.label().to_owned();
    let hint = task.hint();
    let priority = task.priority();
    let poll_after = task.poll_after();
    let configuration = Configuration::new(hint, priority, poll_after);
    let future = task.into_future();
    let t = crate::Task::without_notifications(label.clone(), configuration, async move {
        let r = future.await;
        c.send(r);
    });
    let o = executor.spawn_static(t);
    let name = format!("pin_static_to_thread continuation for {}", label);
    let t = crate::Task::without_notifications(name, configuration, async move { o.await });
    let o2 = executor.spawn_static(t).detach(); //can't hold this across await points

    async { fut.await }
}
