// SPDX-License-Identifier: MIT OR Apache-2.0

//! Support for static executors, primarily for type erasure.
//!
//! This module provides adapter types that allow static executors with different
//! notifier types to be used with a unified interface. This is useful when you
//! need to store executors with different notifier types in the same collection
//! or pass them through a common interface.
//!
//! # Overview
//!
//! Static executors in some_executor have an associated `ExecutorNotifier` type
//! that specifies how the executor receives notifications about task lifecycle
//! events. Different executor implementations may use different notifier types,
//! which can make it difficult to use them interchangeably.
//!
//! The [`OwnedSomeStaticExecutorErasingNotifier`] adapter solves this by wrapping
//! a static executor and erasing its notifier type to `Box<dyn ExecutorNotified>`.
//!
//! # Examples
//!
//! ## Wrapping an executor with type erasure
//!
//! ```
//! use some_executor::static_support::OwnedSomeStaticExecutorErasingNotifier;
//! use some_executor::DynStaticExecutor;
//!
//! // Assuming you have a concrete static executor
//! // let my_executor = MyStaticExecutor::new();
//! // let erased = OwnedSomeStaticExecutorErasingNotifier::new(my_executor);
//! //
//! // Now you can use it as a DynStaticExecutor
//! // let boxed: Box<DynStaticExecutor> = erased.clone_box();
//! ```

use crate::observer::{ExecutorNotified, Observer, ObserverNotified};
use crate::task::Task;
use crate::{
    BoxedStaticObserver, BoxedStaticObserverFuture, DynStaticExecutor, ObjSafeStaticTask,
    SomeStaticExecutor,
};
use std::future::Future;
use std::marker::PhantomData;

/// An adapter that wraps a static executor and erases its notifier type.
///
/// `OwnedSomeStaticExecutorErasingNotifier` takes ownership of an underlying
/// static executor and implements `SomeStaticExecutor` with a type-erased
/// notifier (`Box<dyn ExecutorNotified>`). This allows different executor
/// types to be used interchangeably through the `DynStaticExecutor` type.
///
/// # Type Parameters
///
/// * `UnderlyingExecutor` - The concrete static executor type being wrapped
///
/// # When to Use
///
/// Use this adapter when you need to:
/// - Store executors with different notifier types in the same collection
/// - Pass executors through a uniform interface that accepts `DynStaticExecutor`
/// - Erase the executor's notifier type while maintaining ownership
///
/// # Examples
///
/// ## Creating and using an erased executor
///
/// ```
/// use some_executor::static_support::OwnedSomeStaticExecutorErasingNotifier;
/// use some_executor::{SomeStaticExecutor, DynStaticExecutor};
/// use some_executor::task::{Task, Configuration};
///
/// // With a concrete executor implementation:
/// // let executor = MyExecutor::new();
/// // let mut erased = OwnedSomeStaticExecutorErasingNotifier::new(executor);
/// //
/// // // Spawn tasks as usual
/// // let task = Task::without_notifications(
/// //     "example".to_string(),
/// //     Configuration::default(),
/// //     async { 42 }
/// // );
/// // let observer = erased.spawn_static(task);
/// ```
///
/// ## Converting to a boxed dynamic executor
///
/// ```
/// use some_executor::static_support::OwnedSomeStaticExecutorErasingNotifier;
/// use some_executor::{SomeStaticExecutor, DynStaticExecutor};
///
/// // With a concrete executor:
/// // let executor = MyExecutor::new();
/// // let erased = OwnedSomeStaticExecutorErasingNotifier::new(executor);
/// //
/// // // Get a boxed version for storage or passing around
/// // let boxed: Box<DynStaticExecutor> = erased.clone_box();
/// ```
///
/// # Implementation Notes
///
/// The adapter delegates all executor methods to the underlying executor,
/// with the exception of `executor_notifier()` which wraps the underlying
/// notifier in a `Box<dyn ExecutorNotified>`.
pub struct OwnedSomeStaticExecutorErasingNotifier<UnderlyingExecutor> {
    executor: UnderlyingExecutor,
    _phantom: PhantomData<()>,
}

impl<UnderlyingExecutor> OwnedSomeStaticExecutorErasingNotifier<UnderlyingExecutor> {
    /// Creates a new adapter wrapping the provided executor.
    ///
    /// # Arguments
    ///
    /// * `executor` - The static executor to wrap
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::static_support::OwnedSomeStaticExecutorErasingNotifier;
    ///
    /// // With a concrete executor:
    /// // let executor = MyExecutor::new();
    /// // let erased = OwnedSomeStaticExecutorErasingNotifier::new(executor);
    /// ```
    pub fn new(executor: UnderlyingExecutor) -> Self {
        Self {
            executor,
            _phantom: PhantomData,
        }
    }
}

impl<UnderlyingExecutor: std::fmt::Debug> std::fmt::Debug
    for OwnedSomeStaticExecutorErasingNotifier<UnderlyingExecutor>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedSomeStaticExecutorErasingNotifier")
            .field("executor", &self.executor)
            .finish()
    }
}

impl<UnderlyingExecutor: SomeStaticExecutor> SomeStaticExecutor
    for OwnedSomeStaticExecutorErasingNotifier<UnderlyingExecutor>
{
    type ExecutorNotifier = Box<dyn ExecutorNotified>;

    fn spawn_static<F, Notifier: ObserverNotified<F::Output>>(
        &mut self,
        task: Task<F, Notifier>,
    ) -> impl Observer<Value = F::Output>
    where
        Self: Sized,
        F: Future + 'static,
        F::Output: 'static + Unpin,
    {
        self.executor.spawn_static(task)
    }

    fn spawn_static_async<F, Notifier: ObserverNotified<F::Output>>(
        &mut self,
        task: Task<F, Notifier>,
    ) -> impl Future<Output = impl Observer<Value = F::Output>>
    where
        Self: Sized,
        F: Future + 'static,
        F::Output: 'static + Unpin,
    {
        self.executor.spawn_static_async(task)
    }

    fn spawn_static_objsafe(&mut self, task: ObjSafeStaticTask) -> BoxedStaticObserver {
        self.executor.spawn_static_objsafe(task)
    }

    fn spawn_static_objsafe_async<'s>(
        &'s mut self,
        task: ObjSafeStaticTask,
    ) -> BoxedStaticObserverFuture<'s> {
        #[allow(clippy::async_yields_async)]
        Box::new(async {
            let objsafe_spawn_fut = self.executor.spawn_static_objsafe_async(task);
            Box::into_pin(objsafe_spawn_fut).await
        })
    }

    fn clone_box(&self) -> Box<DynStaticExecutor> {
        // This adapter doesn't support cloning. Use StaticExecutorExt for Clone support.
        self.executor.clone_box()
    }

    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        self.executor
            .executor_notifier()
            .map(|notifier| Box::new(notifier) as Box<dyn ExecutorNotified>)
    }
}
