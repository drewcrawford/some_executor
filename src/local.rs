//SPDX-License-Identifier: MIT OR Apache-2.0

use std::any::Any;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use crate::observer::{ExecutorNotified, TypedObserver, ObserverNotified};
use crate::{SomeLocalExecutor};
use crate::task::Task;

pub(crate) struct SomeLocalExecutorErasingNotifier<'borrow, 'underlying, UnderlyingExecutor: SomeLocalExecutor<'underlying> + ?Sized> {
    executor: &'borrow mut UnderlyingExecutor,
    _phantom: PhantomData<&'underlying ()>
}

impl <'borrow, 'underlying, UnderlyingExecutor: SomeLocalExecutor<'underlying> + ?Sized> SomeLocalExecutorErasingNotifier<'borrow, 'underlying, UnderlyingExecutor> {
    pub(crate) fn new(executor: &'borrow mut UnderlyingExecutor) -> Self {
        Self {
            executor,
            _phantom: PhantomData
        }
    }
}

impl<'borrow, 'executor, UnderlyingExecutor: SomeLocalExecutor<'executor>> SomeLocalExecutor<'executor> for SomeLocalExecutorErasingNotifier<'borrow, 'executor, UnderlyingExecutor> {
    type ExecutorNotifier = Box<dyn ExecutorNotified>;

    fn spawn_local<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F, Notifier>) -> TypedObserver<F::Output, Self::ExecutorNotifier>
    where
        Self: Sized,
        F: 'executor,
    /* I am a little uncertain whether this is really required */
        <F as Future>::Output: Unpin
    /*I am a little uncertain as to whether this is really required */
    {
        let o = self.executor.spawn_local(task);
        o.into_boxed_notifier()
    }

    fn spawn_local_async<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F, Notifier>) -> impl Future<Output=TypedObserver<F::Output, Self::ExecutorNotifier>>
    where
        Self: Sized,
        F: 'executor
    {
        async {
            let o = self.executor.spawn_local_async(task).await;
            o.into_boxed_notifier()
        }
    }


    fn spawn_local_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> TypedObserver<Box<dyn Any>, Box<dyn ExecutorNotified>> {
        let o = self.executor.spawn_local_objsafe(task);
        o.into_boxed_notifier()
    }

    fn spawn_local_objsafe_async<'s>(&'s mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Future<Output=TypedObserver<Box<dyn Any>, Box<dyn ExecutorNotified>>> + 's> {
        Box::new(async {
            let objsafe_spawn_fut = self.executor.spawn_local_objsafe_async(task);
            let result = Box::into_pin(objsafe_spawn_fut).await;
            result.into_boxed_notifier()
        })
    }


    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        self.executor.executor_notifier().map(|x| Box::new(x) as Self::ExecutorNotifier)
    }
}

pub (crate) struct UnsafeErasedLocalExecutor {
    underlying: *mut dyn SomeLocalExecutor<'static, ExecutorNotifier=Box<dyn ExecutorNotified>>,
}

impl UnsafeErasedLocalExecutor {
    /**
    Creates a new type.

    # Safety

    The underlying executor must be valid for the lifetime of this type.
    */
    pub unsafe fn new<'e>(underlying:&mut (dyn SomeLocalExecutor<ExecutorNotifier=Box<dyn ExecutorNotified>> + 'e)) -> Self {
        Self {
            underlying: std::mem::transmute(underlying),
        }
    }

    fn executor(&mut self) -> &mut (dyn SomeLocalExecutor<ExecutorNotifier=Box<dyn ExecutorNotified>> + '_) {
        // Safety: `underlying` is assumed to be valid for the duration of `&mut self`
        unsafe {
            std::mem::transmute::<
                &mut dyn SomeLocalExecutor<ExecutorNotifier=Box<dyn ExecutorNotified>>,
                &mut dyn SomeLocalExecutor<ExecutorNotifier=Box<dyn ExecutorNotified>>
            >(&mut *self.underlying)
        }
    }
}

impl<'a> SomeLocalExecutor<'a> for UnsafeErasedLocalExecutor {
    type ExecutorNotifier = Box<dyn ExecutorNotified>;

    fn spawn_local<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, _task: Task<F, Notifier>) -> TypedObserver<F::Output, Self::ExecutorNotifier>
    where
        Self: Sized,
        F: 'a,
    /* I am a little uncertain whether this is really required */
        <F as Future>::Output: Unpin
    {
        unimplemented!("Not implemented for erased executor; use objsafe method")
    }

    fn spawn_local_async<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, _task: Task<F, Notifier>) -> impl Future<Output=TypedObserver<F::Output, Self::ExecutorNotifier>>
    where
        Self: Sized
    {
        async { unimplemented!("Not implemented for erased executor; use objsafe method") }
    }

    fn spawn_local_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> TypedObserver<Box<dyn Any>, Box<dyn ExecutorNotified>> {
        let ex = self.executor();
        ex.spawn_local_objsafe(task)
    }

    fn spawn_local_objsafe_async<'s>(&'s mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Future<Output=TypedObserver<Box<dyn Any>, Box<dyn ExecutorNotified>>> + 's> {
        let ex = self.executor();
        ex.spawn_local_objsafe_async(task)
    }

    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        self.executor().executor_notifier()
    }
}