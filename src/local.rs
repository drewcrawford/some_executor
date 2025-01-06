//SPDX-License-Identifier: MIT OR Apache-2.0

use std::any::Any;
use std::convert::Infallible;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use crate::observer::{ExecutorNotified, TypedObserver, ObserverNotified, Observer, FinishedObservation};
use crate::{SomeLocalExecutor};
use crate::task::Task;

/**
Erases the executor notifier type
*/
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

    fn spawn_local<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F, Notifier>) -> impl Observer<Value=F::Output>
    where
        Self: Sized,
        F: 'executor,
    /* I am a little uncertain whether this is really required */
        <F as Future>::Output: Unpin,
        <F as Future>::Output: 'static,
    {
        self.executor.spawn_local(task)
    }

    fn spawn_local_async<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F, Notifier>) -> impl Future<Output=impl Observer<Value=F::Output>>
    where
        Self: Sized,
        F: 'executor,
        F::Output: 'static + Unpin,
    {
        async {
            self.executor.spawn_local_async(task).await
        }
    }


    fn spawn_local_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Observer<Value=Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>> {
        self.executor.spawn_local_objsafe(task)
    }

    fn spawn_local_objsafe_async<'s>(&'s mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Future<Output=Box<dyn Observer<Value=Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>>> + 's> {
        Box::new(async {
            let objsafe_spawn_fut = self.executor.spawn_local_objsafe_async(task);
            Box::into_pin(objsafe_spawn_fut).await
        })
    }


    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        self.executor.executor_notifier().map(|x| Box::new(x) as Self::ExecutorNotifier)
    }
}

/**
Like `SomeLocalExecutorErasingNotifier`, but owns the underlying executor.
*/

pub(crate) struct OwnedSomeLocalExecutorErasingNotifier<'underlying, UnderlyingExecutor> {
    executor: UnderlyingExecutor,
    _phantom: PhantomData<&'underlying ()>
}

impl<'underlying,UnderlyingExecutor> OwnedSomeLocalExecutorErasingNotifier<'underlying,UnderlyingExecutor> {
    pub(crate) fn new(executor: UnderlyingExecutor) -> Self {
        Self {
            executor,
            _phantom: PhantomData
        }
    }
}

impl <'underlying, UnderlyingExecutor: SomeLocalExecutor<'underlying>> SomeLocalExecutor<'underlying> for OwnedSomeLocalExecutorErasingNotifier<'underlying, UnderlyingExecutor> {
    type ExecutorNotifier = Box<dyn ExecutorNotified>;

    fn spawn_local<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F, Notifier>) -> impl Observer<Value=F::Output>
    where
        Self: Sized,
        F: 'underlying,
    /* I am a little uncertain whether this is really required */
        <F as Future>::Output: Unpin,
        <F as Future>::Output: 'static,
    {
        self.executor.spawn_local(task)
    }

    fn spawn_local_async<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F, Notifier>) -> impl Future<Output=impl Observer<Value=F::Output>>
    where
        Self: Sized,
        F: 'underlying,
        F::Output: 'static + Unpin,
    {
        async {
            self.executor.spawn_local_async(task).await
        }
    }

    fn spawn_local_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Observer<Value=Box<dyn Any>, Output=FinishedObservation<Box<dyn Any>>>> {
        self.executor.spawn_local_objsafe(task)
    }

    fn spawn_local_objsafe_async<'s>(&'s mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Future<Output=Box<dyn Observer<Value=Box<dyn Any>, Output=FinishedObservation<Box<dyn Any>>>>> + 's> {
        Box::new(async {
            let objsafe_spawn_fut = self.executor.spawn_local_objsafe_async(task);
            Box::into_pin(objsafe_spawn_fut).await
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

    fn spawn_local<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, _task: Task<F, Notifier>) -> impl Observer<Value=F::Output>
    where
        Self: Sized,
        F: 'a,
    /* I am a little uncertain whether this is really required */
        <F as Future>::Output: Unpin,
        <F as Future>::Output: 'static,
    {
        #[allow(unreachable_code)] {
            unimplemented!("Not implemented for erased executor; use objsafe method") as TypedObserver<F::Output, Infallible>
        }
    }

    fn spawn_local_async<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, _task: Task<F, Notifier>) -> impl Future<Output=impl Observer<Value=F::Output>>
    where
        Self: Sized,
        F::Output: 'static,
    {
        #[allow(unreachable_code)] {
            async { unimplemented!("Not implemented for erased executor; use objsafe method") as TypedObserver<F::Output, Infallible> }
        }
    }

    fn spawn_local_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Observer<Value=Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>> {
        let ex = self.executor();
        ex.spawn_local_objsafe(task)
    }

    fn spawn_local_objsafe_async<'s>(&'s mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Future<Output=Box<dyn Observer<Value=Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>>> + 's> {
        let ex = self.executor();
        ex.spawn_local_objsafe_async(task)
    }

    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        self.executor().executor_notifier()
    }
}