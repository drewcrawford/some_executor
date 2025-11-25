// SPDX-License-Identifier: MIT OR Apache-2.0

//! Dynamic executor implementations with type erasure support.
//!
//! This module provides implementations that enable trait objects (`Box<dyn SomeExecutor>`)
//! to function as executors themselves. This is crucial for storing executors in collections
//! or passing them around without knowing their concrete types at compile time.
//!
//! # Overview
//!
//! The type erasure pattern implemented here allows you to use `Box<dyn SomeExecutor>`
//! as if it were a concrete executor type. This is particularly useful when:
//!
//! - You need to store multiple executors of different types
//! - You want to pass executors across API boundaries without generic parameters
//! - You're building runtime-configurable executor selection
//!
//! # How It Works
//!
//! The module implements the executor traits for boxed trait objects, enabling them to:
//! 1. Accept typed futures and convert them to type-erased versions
//! 2. Spawn these type-erased futures onto the underlying executor
//! 3. Return observers that automatically downcast results back to the original type
//!
//! # Type Safety
//!
//! Despite the type erasure, the implementation maintains type safety through:
//! - Compile-time type checking when spawning tasks
//! - Runtime downcasting that panics on type mismatches (programming errors)
//! - Phantom types to track the original output type
//!
//! # Examples
//!
//! ## Using a boxed executor
//!
//! ```
//! use some_executor::{SomeExecutor, task::{Task, Configuration}};
//! use std::future::Future;
//! use std::convert::Infallible;
//!
//! # fn example() {
//! // Store any executor as a trait object
//! let mut boxed_executor: Box<dyn SomeExecutor<ExecutorNotifier = Infallible>> =
//!     todo!(); // Would come from a real executor implementation
//!
//! // Use it just like a concrete executor
//! let task = Task::without_notifications(
//!     "example".to_string(),
//!     Configuration::default(),
//!     async { 42 }
//! );
//! # // Can't actually spawn without a real executor
//! # // let observer = boxed_executor.spawn(task);
//! # }
//! ```
//!
//! ## Storing multiple executor types
//!
//! ```
//! use some_executor::SomeExecutor;
//! use std::convert::Infallible;
//!
//! struct ExecutorPool {
//!     executors: Vec<Box<dyn SomeExecutor<ExecutorNotifier = Infallible>>>,
//! }
//!
//! impl ExecutorPool {
//!     fn add_executor(&mut self, executor: Box<dyn SomeExecutor<ExecutorNotifier = Infallible>>) {
//!         self.executors.push(executor);
//!     }
//!     
//!     fn get_executor(&mut self) -> Option<&mut Box<dyn SomeExecutor<ExecutorNotifier = Infallible>>> {
//!         self.executors.first_mut()
//!     }
//! }
//! ```

use crate::dyn_observer::DowncastObserver;
use crate::observer::{
    ExecutorNotified, FinishedObservation, Observation, Observer, ObserverNotified,
};
use crate::task::{Task, TaskID};
use crate::{DynExecutor, DynStaticExecutor, SomeExecutor, SomeStaticExecutor};
use std::any::Any;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Observer that downcasts type-erased results back to their original type for static executors.
///
/// This is the non-Send version of [`DowncastObserver`], used with [`SomeStaticExecutor`]
/// implementations that work with `!Send` futures. It performs runtime downcasting of
/// `Box<dyn Any + 'static>` values back to the concrete type `V`.
///
/// # Type Parameters
///
/// - `O`: The underlying observer type that produces `Box<dyn Any + 'static>` values
/// - `V`: The concrete type to downcast to
///
/// # Panics
///
/// Methods will panic if downcasting fails, which indicates a type mismatch bug in the
/// executor implementation.
pub struct StaticDowncastObserver<O, V>(O, PhantomData<V>);

impl<O, V> Observer for StaticDowncastObserver<O, V>
where
    O: Observer<Value = Box<dyn Any + 'static>> + 'static,
    V: 'static,
{
    type Value = V;

    /// Observes the current state of the task and downcasts the result if ready.
    ///
    /// # Panics
    ///
    /// Panics if the task result is ready but cannot be downcast to type `V`.
    /// This indicates a type system violation in the executor implementation.
    fn observe(&self) -> Observation<Self::Value> {
        let inner = self.0.observe();
        match inner {
            Observation::Pending => Observation::Pending,
            Observation::Ready(e) => {
                let downcasted = e.downcast::<V>().expect("Downcast failed");
                Observation::Ready(*downcasted)
            }
            Observation::Done => Observation::Done,
            Observation::Cancelled => Observation::Cancelled,
        }
    }

    /// Returns the task ID of the underlying observer.
    fn task_id(&self) -> &TaskID {
        self.0.task_id()
    }
}

impl<O, V> StaticDowncastObserver<O, V> {
    /// Creates a new `StaticDowncastObserver` wrapping the given observer.
    ///
    /// # Arguments
    ///
    /// * `observer` - The underlying observer that produces type-erased values
    ///
    /// # Examples
    ///
    /// ```compile_fail
    /// // ALLOW_COMPILE_FAIL_DOCTEST: This shows the API but requires internal types
    /// // The StaticDowncastObserver is not exported publicly
    /// use some_executor::dyn_executor::StaticDowncastObserver;
    /// use some_executor::observer::{Observer, Observation};
    /// use std::any::Any;
    ///
    /// struct MockObserver;
    /// // Observer requires Future trait which MockObserver doesn't implement
    /// // This is just to show the API usage
    /// let mock_observer = /* ... */;
    /// let typed_observer: StaticDowncastObserver<_, i32> =
    ///     StaticDowncastObserver::new(mock_observer);
    /// ```
    pub fn new(observer: O) -> Self {
        Self(observer, PhantomData)
    }
}

impl<O, V> Future for StaticDowncastObserver<O, V>
where
    O: Future<Output = FinishedObservation<Box<dyn Any + 'static>>> + 'static,
    V: 'static,
{
    type Output = FinishedObservation<V>;

    /// Polls the underlying future and downcasts the result if ready.
    ///
    /// # Panics
    ///
    /// Panics if the future completes with a value that cannot be downcast to type `V`.
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let f = unsafe { self.map_unchecked_mut(|s| &mut s.0) }.poll(cx);
        match f {
            Poll::Pending => Poll::Pending,
            Poll::Ready(e) => match e {
                FinishedObservation::Cancelled => Poll::Ready(FinishedObservation::Cancelled),
                FinishedObservation::Ready(e) => {
                    let downcasted = e.downcast::<V>().expect("Downcast failed");
                    Poll::Ready(FinishedObservation::Ready(*downcasted))
                }
            },
        }
    }
}

/// Implementation of `SomeExecutor` for boxed executor trait objects.
///
/// This implementation enables `Box<dyn SomeExecutor>` to act as an executor itself,
/// providing the crucial type erasure pattern that allows executors to be stored and
/// used polymorphically.
///
/// # How It Works
///
/// When you spawn a task on a boxed executor:
/// 1. The concrete future type is converted to a type-erased version using `into_objsafe()`
/// 2. The type-erased task is spawned on the underlying executor
/// 3. A `DowncastObserver` is returned that will convert results back to the original type
///
/// # Examples
///
/// ```
/// # use some_executor::{SomeExecutor, task::{Task, Configuration}};
/// # use std::convert::Infallible;
/// # fn example() {
/// let mut executor: Box<dyn SomeExecutor<ExecutorNotifier = Infallible>> = todo!();
///
/// // Spawn a typed future
/// let task = Task::without_notifications(
///     "hello-task".to_string(),
///     Configuration::default(),
///     async { "hello".to_string() }
/// );
/// # // Would work with a real executor:
/// # // let observer = executor.spawn(task);
/// # // assert_eq!(observer.await.unwrap(), "hello");
/// # }
/// ```
impl<UnderlyingNotifier: ExecutorNotified + Send> SomeExecutor
    for Box<dyn SomeExecutor<ExecutorNotifier = UnderlyingNotifier>>
{
    type ExecutorNotifier = Box<dyn ExecutorNotified + Send>;

    /// Spawns a future onto the underlying executor with automatic type erasure and recovery.
    ///
    /// This method handles the type erasure process transparently, converting the typed
    /// future into a type-erased version for the underlying executor, then wrapping the
    /// returned observer to restore type information.
    fn spawn<F: Future + Send + 'static, Notifier: ObserverNotified<F::Output> + Send>(
        &mut self,
        task: Task<F, Notifier>,
    ) -> impl Observer<Value = F::Output>
    where
        Self: Sized,
        F::Output: Send + Unpin,
    {
        let underlying = self.as_mut();
        let objsafe = task.into_objsafe();
        let observer = underlying.spawn_objsafe(objsafe);
        // Restore the concrete type through downcasting
        let downcasted: DowncastObserver<_, F::Output> = DowncastObserver::new(observer);
        downcasted
    }

    /// Asynchronously spawns a future onto the underlying executor.
    ///
    /// Similar to `spawn`, but allows the spawning process itself to be asynchronous.
    async fn spawn_async<F: Future + Send + 'static, Notifier: ObserverNotified<F::Output> + Send>(
        &mut self,
        task: Task<F, Notifier>,
    ) -> impl Observer<Value = F::Output>
    where
        Self: Sized,
        F::Output: Send + Unpin,
    {
        let underlying = self.as_mut();
        let objsafe = task.into_objsafe();
        let observer = underlying.spawn_objsafe(objsafe);
        // Restore the concrete type through downcasting
        let downcasted: DowncastObserver<_, F::Output> = DowncastObserver::new(observer);
        downcasted
    }

    /// Spawns an already type-erased task.
    ///
    /// This is the object-safe method that works with type-erased futures directly.
    /// User code typically doesn't call this; it's used internally by the typed spawn methods.
    fn spawn_objsafe(
        &mut self,
        task: Task<
            Pin<Box<dyn Future<Output = Box<dyn Any + 'static + Send>> + 'static + Send>>,
            Box<dyn ObserverNotified<dyn Any + Send> + Send>,
        >,
    ) -> Box<
        dyn Observer<Value = Box<dyn Any + Send>, Output = FinishedObservation<Box<dyn Any + Send>>>
            + Send,
    > {
        self.as_mut().spawn_objsafe(task)
    }

    /// Asynchronously spawns an already type-erased task.
    ///
    /// The async version of `spawn_objsafe` for executors that need async initialization.
    fn spawn_objsafe_async<'s>(
        &'s mut self,
        task: Task<
            Pin<Box<dyn Future<Output = Box<dyn Any + 'static + Send>> + 'static + Send>>,
            Box<dyn ObserverNotified<dyn Any + Send> + Send>,
        >,
    ) -> Box<
        dyn Future<
                Output = Box<
                    dyn Observer<
                            Value = Box<dyn Any + Send>,
                            Output = FinishedObservation<Box<dyn Any + Send>>,
                        > + Send,
                >,
            > + 's,
    > {
        self.as_mut().spawn_objsafe_async(task)
    }

    /// Creates a boxed clone of the executor.
    ///
    /// This allows the boxed executor to be cloned while preserving the type erasure.
    fn clone_box(&self) -> Box<DynExecutor> {
        self.as_ref().clone_box()
    }

    /// Gets a notifier for executor-level events, if supported.
    ///
    /// Returns a type-erased notifier that can be used to wake the executor.
    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        let underlying = self.as_mut().executor_notifier();
        underlying.map(|u| Box::new(u) as Box<dyn ExecutorNotified + Send>)
    }
}

/// Implementation of `SomeStaticExecutor` for boxed static executor trait objects.
///
/// This implementation enables `Box<dyn SomeStaticExecutor>` to act as a static executor,
/// supporting `!Send` futures through type erasure. This is particularly useful for
/// single-threaded or local executors.
///
/// # Differences from `SomeExecutor`
///
/// Unlike the `SomeExecutor` implementation, this works with futures that are not `Send`,
/// making it suitable for JavaScript/WASM environments or thread-local execution contexts.
///
/// # Examples
///
/// ```
/// # use some_executor::{SomeStaticExecutor, task::{Task, Configuration}};
/// # use std::convert::Infallible;
/// # use std::rc::Rc;
/// # fn example() {
/// let mut executor: Box<dyn SomeStaticExecutor<ExecutorNotifier = Infallible>> = todo!();
///
/// // Can spawn !Send futures
/// let rc = Rc::new(42);
/// let task = Task::without_notifications(
///     "rc-task".to_string(),
///     Configuration::default(),
///     async move { *rc }
/// );
/// # // Would work with a real executor:
/// # // let observer = executor.spawn_static(task);
/// # }
/// ```
impl<UnderlyingNotifier: ExecutorNotified> SomeStaticExecutor
    for Box<dyn SomeStaticExecutor<ExecutorNotifier = UnderlyingNotifier>>
{
    type ExecutorNotifier = Box<dyn ExecutorNotified>;

    /// Spawns a static (potentially !Send) future onto the underlying executor.
    ///
    /// This method supports futures that don't implement `Send`, making it suitable
    /// for single-threaded environments.
    fn spawn_static<F, Notifier: ObserverNotified<F::Output>>(
        &mut self,
        task: Task<F, Notifier>,
    ) -> impl Observer<Value = F::Output>
    where
        Self: Sized,
        F: Future + 'static,
        F::Output: 'static + Unpin,
    {
        let underlying = self.as_mut();
        let objsafe = task.into_objsafe_static();
        let observer = underlying.spawn_static_objsafe(objsafe);
        // Use StaticDowncastObserver for non-Send type recovery
        StaticDowncastObserver::new(observer)
    }

    /// Asynchronously spawns a static future onto the underlying executor.
    ///
    /// The async version of `spawn_static` for executors that need async initialization.
    async fn spawn_static_async<F, Notifier: ObserverNotified<F::Output>>(
        &mut self,
        task: Task<F, Notifier>,
    ) -> impl Observer<Value = F::Output>
    where
        Self: Sized,
        F: Future + 'static,
        F::Output: 'static + Unpin,
    {
        let underlying = self.as_mut();
        let objsafe = task.into_objsafe_static();
        let observer = underlying.spawn_static_objsafe(objsafe);
        StaticDowncastObserver::new(observer)
    }

    /// Spawns an already type-erased static task.
    ///
    /// This is the object-safe method for static executors. User code typically
    /// doesn't call this directly.
    fn spawn_static_objsafe(
        &mut self,
        task: crate::ObjSafeStaticTask,
    ) -> crate::BoxedStaticObserver {
        self.as_mut().spawn_static_objsafe(task)
    }

    /// Asynchronously spawns an already type-erased static task.
    ///
    /// The async version of `spawn_static_objsafe`.
    fn spawn_static_objsafe_async<'s>(
        &'s mut self,
        task: crate::ObjSafeStaticTask,
    ) -> crate::BoxedStaticObserverFuture<'s> {
        self.as_mut().spawn_static_objsafe_async(task)
    }

    /// Creates a boxed clone of the static executor.
    fn clone_box(&self) -> Box<DynStaticExecutor> {
        self.as_ref().clone_box()
    }

    /// Gets a notifier for executor-level events, if supported.
    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        let underlying = self.as_mut().executor_notifier();
        underlying.map(|u| Box::new(u) as Box<dyn ExecutorNotified>)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::{Infallible, SomeExecutor};

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::*;

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn test_dyn_executor() {
        //mt2-697
        #[allow(dead_code)]
        fn just_compile() {
            let _ty: Box<dyn SomeExecutor<ExecutorNotifier = Infallible>> = todo!();
            #[allow(unreachable_code, unused_variables)]
            fn expect_executor<E: SomeExecutor>(e: E) {}
            #[allow(unreachable_code)]
            expect_executor(_ty);
        }
    }
}
