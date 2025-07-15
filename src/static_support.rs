// SPDX-License-Identifier: MIT OR Apache-2.0

/*!
Support for static executors, primarily for type erasure.

This module provides adapter types that allow static executors with different
notifier types to be used with a unified interface.
*/

use crate::observer::{ExecutorNotified, Observer, ObserverNotified};
use crate::task::Task;
use crate::{
    BoxedStaticObserver, BoxedStaticObserverFuture, DynStaticExecutor, ObjSafeStaticTask,
    SomeStaticExecutor,
};
use std::future::Future;
use std::marker::PhantomData;

/**
Like `SomeStaticExecutorErasingNotifier`, but owns the underlying executor.
*/
pub(crate) struct OwnedSomeStaticExecutorErasingNotifier<UnderlyingExecutor> {
    executor: UnderlyingExecutor,
    _phantom: PhantomData<()>,
}

impl<UnderlyingExecutor> OwnedSomeStaticExecutorErasingNotifier<UnderlyingExecutor> {
    pub(crate) fn new(executor: UnderlyingExecutor) -> Self {
        Self {
            executor,
            _phantom: PhantomData,
        }
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
        unimplemented!()
    }

    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        self.executor
            .executor_notifier()
            .map(|notifier| Box::new(notifier) as Box<dyn ExecutorNotified>)
    }
}
