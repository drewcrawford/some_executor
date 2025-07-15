// SPDX-License-Identifier: MIT OR Apache-2.0

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

/**
A downcast observer for static executors that handles `Box<dyn Any + 'static>` instead of `Box<dyn Any + Send + 'static>`.
This is similar to `DowncastObserver` but for non-Send types.
*/
pub struct StaticDowncastObserver<O, V>(O, PhantomData<V>);

impl<O, V> Observer for StaticDowncastObserver<O, V>
where
    O: Observer<Value = Box<dyn Any + 'static>> + 'static,
    V: 'static,
{
    type Value = V;

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

    fn task_id(&self) -> &TaskID {
        self.0.task_id()
    }
}

impl<O, V> StaticDowncastObserver<O, V> {
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

impl<UnderlyingNotifier: ExecutorNotified + Send> SomeExecutor
    for Box<dyn SomeExecutor<ExecutorNotifier = UnderlyingNotifier>>
{
    type ExecutorNotifier = Box<dyn ExecutorNotified + Send>;

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
        //write in the type again
        let downcasted: DowncastObserver<_, F::Output> = DowncastObserver::new(observer);
        downcasted
    }

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
        //write in the type again
        let downcasted: DowncastObserver<_, F::Output> = DowncastObserver::new(observer);
        downcasted
    }

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

    fn clone_box(&self) -> Box<DynExecutor> {
        self.as_ref().clone_box()
    }

    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        let underlying = self.as_mut().executor_notifier();
        underlying.map(|u| Box::new(u) as Box<dyn ExecutorNotified + Send>)
    }
}

impl<UnderlyingNotifier: ExecutorNotified> SomeStaticExecutor
    for Box<dyn SomeStaticExecutor<ExecutorNotifier = UnderlyingNotifier>>
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
        let underlying = self.as_mut();
        let objsafe = task.into_objsafe_static();
        let observer = underlying.spawn_static_objsafe(objsafe);
        // For static executors, we need to create a non-Send version of downcast observer
        // For now, return the observer directly since it's object-safe
        StaticDowncastObserver::new(observer)
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
        #[allow(clippy::async_yields_async)]
        async move {
            let underlying = self.as_mut();
            let objsafe = task.into_objsafe_static();
            let observer = underlying.spawn_static_objsafe(objsafe);
            StaticDowncastObserver::new(observer)
        }
    }

    fn spawn_static_objsafe(
        &mut self,
        task: crate::ObjSafeStaticTask,
    ) -> crate::BoxedStaticObserver {
        self.as_mut().spawn_static_objsafe(task)
    }

    fn spawn_static_objsafe_async<'s>(
        &'s mut self,
        task: crate::ObjSafeStaticTask,
    ) -> crate::BoxedStaticObserverFuture<'s> {
        self.as_mut().spawn_static_objsafe_async(task)
    }

    fn clone_box(&self) -> Box<DynStaticExecutor> {
        self.as_ref().clone_box()
    }

    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        let underlying = self.as_mut().executor_notifier();
        underlying.map(|u| Box::new(u) as Box<dyn ExecutorNotified>)
    }
}

#[cfg(test)]
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
