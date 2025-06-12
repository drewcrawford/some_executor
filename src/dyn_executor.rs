//SPDX-License-Identifier: MIT OR Apache-2.0

use crate::dyn_observer::DowncastObserver;
use crate::observer::{ExecutorNotified, FinishedObservation, Observer, ObserverNotified};
use crate::task::Task;
use crate::{DynExecutor, SomeExecutor};
use std::any::Any;
use std::future::Future;
use std::pin::Pin;

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

#[cfg(test)]
mod tests {
    use crate::{Infallible, SomeExecutor};
    #[test]
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
