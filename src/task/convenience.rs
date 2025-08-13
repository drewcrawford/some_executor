// SPDX-License-Identifier: MIT OR Apache-2.0

//! Convenience methods for Task types.
//!
//! This module provides convenience methods that make common task operations simpler,
//! particularly for tasks that output `()` and for bridging between Send and !Send contexts.

use super::{Configuration, Task};
use crate::SomeExecutor;
use crate::observer::{Observer, ObserverNotified};
use std::convert::Infallible;
use std::future::Future;
use std::task::Poll;

impl<F: Future<Output = ()>, N> Task<F, N> {
    /// Spawns the task onto the current executor and detaches the observer.
    ///
    /// This is a convenience method for fire-and-forget tasks that don't return a value.
    /// The task will be spawned using [`current_executor`](crate::current_executor::current_executor)
    /// and the observer will be automatically detached.
    ///
    /// # Requirements
    ///
    /// - The future must output `()`
    /// - The future must be `Send + 'static`
    /// - The notifier must be `Send + 'static`
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration};
    ///
    /// # async fn example() {
    /// let task = Task::without_notifications(
    ///     "background-work".to_string(),
    ///     Configuration::default(),
    ///     async {
    ///         println!("Running in background");
    ///     },
    /// );
    ///
    /// // Spawn and forget
    /// task.spawn_current();
    /// # }
    /// ```
    pub fn spawn_current(self)
    where
        F: Send + 'static,
        N: ObserverNotified<()> + Send + 'static,
    {
        let mut executor = crate::current_executor::current_executor();
        executor.spawn(self).detach();
    }

    /// Spawns the task onto the current thread-local executor and detaches the observer.
    ///
    /// This is a convenience method for fire-and-forget tasks that don't return a value
    /// and may not be `Send`. The task will be spawned using the current thread-local executor
    /// and the observer will be automatically detached.
    ///
    /// # Requirements
    ///
    /// - The future must output `()`
    /// - The future must be `'static` (but doesn't need to be `Send`)
    /// - The notifier must implement `ObserverNotified<()>`
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration};
    /// use std::rc::Rc;
    ///
    /// # fn example() {
    /// // Rc is !Send
    /// let data = Rc::new(42);
    /// let data_clone = data.clone();
    ///
    /// let task = Task::without_notifications(
    ///     "local-work".to_string(),
    ///     Configuration::default(),
    ///     async move {
    ///         println!("Local data: {}", data_clone);
    ///     },
    /// );
    ///
    /// // Spawn on current thread-local executor
    /// task.spawn_local_current();
    /// # }
    /// ```
    pub fn spawn_local_current(self)
    where
        F: 'static,
        N: ObserverNotified<()> + 'static,
    {
        todo!("Not yet implemented");
    }

    /// Spawns the task onto the current thread's static executor and detaches the observer.
    ///
    /// This is a convenience method for fire-and-forget tasks that don't return a value
    /// and need to be executed on the current thread's static executor. The task will be
    /// spawned using [`thread_static_executor`](crate::thread_executor::thread_static_executor)
    /// and the observer will be automatically detached.
    ///
    /// # Requirements
    ///
    /// - The future must output `()`
    /// - The future must be `'static` (but doesn't need to be `Send`)
    /// - The notifier must implement `ObserverNotified<()>` and be `'static`
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration};
    ///
    /// # fn example() {
    /// let task = Task::without_notifications(
    ///     "static-work".to_string(),
    ///     Configuration::default(),
    ///     async {
    ///         println!("Running on static executor");
    ///     },
    /// );
    ///
    /// // Spawn on the thread's static executor
    /// task.spawn_static_current();
    /// # }
    /// ```
    pub fn spawn_static_current(self)
    where
        F: 'static,
        N: ObserverNotified<()> + 'static,
    {
        crate::thread_executor::thread_static_executor(|executor| {
            executor
                .clone_box()
                .spawn_static_objsafe(self.into_objsafe_static())
                .detach();
        });
    }
}

impl<F: Future, N> Task<F, N> {
    /// Pins a task to run on the current thread, converting non-Send futures to Send futures.
    ///
    /// This method allows you to work with non-Send types (like `Rc`, `RefCell`) in an async
    /// context by ensuring the task runs only on the current thread.
    ///
    /// # When to Use
    ///
    /// Use this when:
    /// - You need to work with non-Send types across await points
    /// - The executor type is erased or doesn't support non-Send directly
    /// - Converting to Send types isn't feasible
    ///
    /// # Trade-offs
    ///
    /// **Pros:**
    /// - Enables use of non-Send types in async code
    /// - Works with any executor type
    ///
    /// **Cons:**
    /// - Task cannot be moved between threads (less efficient)
    /// - Small runtime overhead for the Send/!Send bridge
    /// - Task spawns immediately (before the returned future is polled)
    /// - Limited cancellation support
    ///
    /// # Alternatives
    ///
    /// Consider these alternatives before using this method:
    ///
    /// 1. Use Send/Sync types where possible (`Arc` instead of `Rc`)
    /// 2. Scope non-Send types to avoid holding them across await points
    ///    (see <https://rust-lang.github.io/async-book/07_workarounds/03_send_approximation.html>)
    /// 3. Use [`SomeStaticExecutor`](crate::SomeStaticExecutor) methods directly if the concrete type supports it
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration};
    /// use std::rc::Rc;
    ///
    /// # async fn example() {
    /// // Rc is !Send
    /// let data = Rc::new(vec![1, 2, 3]);
    /// let data_clone = data.clone();
    ///
    /// let task = Task::without_notifications(
    ///     "process-local-data".to_string(),
    ///     Configuration::default(),
    ///     async move {
    ///         // Can use Rc across await points
    ///         println!("Data: {:?}", data_clone);
    ///         data_clone.len()
    ///     }
    /// );
    ///
    /// // Convert to Send future that runs on current thread
    /// let send_future = task.pin_current();
    /// let result = send_future.await;
    /// assert_eq!(result, 3);
    /// # }
    /// ```
    ///
    /// # See Also
    ///
    /// - [`crate::thread_executor::pin_static_to_thread`] for a more general-purpose pinning function
    pub fn pin_current(self) -> impl Future<Output = F::Output> + Send
    where
        F: 'static,
        F::Output: Send,
    {
        let f = {
            let mut executor = crate::thread_executor::thread_static_executor(|e| e.clone_box());
            crate::thread_executor::pin_static_to_thread(&mut executor, self)
        };
        async { f.await }
    }
}

/// A trivial future that completes immediately with `()`.
///
/// `DefaultFuture` is useful for creating default task instances or for testing
/// purposes where you need a simple, non-blocking future.
///
/// # Examples
///
/// ```
/// use some_executor::task::{DefaultFuture, Task};
/// use std::convert::Infallible;
/// use std::task::Poll;
/// use std::future::Future;
///
/// // DefaultFuture always returns Ready(())
/// # use std::task::Context;
/// # use std::pin::Pin;
/// # fn make_waker() -> std::task::Waker {
/// #     use std::task::{RawWaker, RawWakerVTable, Waker};
/// #     unsafe fn no_op(_: *const ()) {}
/// #     unsafe fn clone(_: *const ()) -> RawWaker {
/// #         RawWaker::new(std::ptr::null(), &VTABLE)
/// #     }
/// #     const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
/// #     unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
/// # }
/// # let waker = make_waker();
/// # let mut cx = Context::from_waker(&waker);
/// let mut future = DefaultFuture;
/// assert_eq!(Pin::new(&mut future).poll(&mut cx), Poll::Ready(()));
///
/// // Can be used to create a default task
/// let task: Task<DefaultFuture, Infallible> = Task::default();
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
pub struct DefaultFuture;

impl Future for DefaultFuture {
    type Output = ();
    fn poll(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Poll::Ready(())
    }
}

impl Default for Task<DefaultFuture, Infallible> {
    fn default() -> Self {
        Task::with_notifications(
            "".to_string(),
            Configuration::default(),
            None,
            DefaultFuture,
        )
    }
}
