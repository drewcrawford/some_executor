use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use crate::observer::{ExecutorNotified, Observer, ObserverNotified, TypedObserver};
use crate::{DynExecutor, SomeExecutor};
use crate::dyn_observer::DowncastObserver;
use crate::task::Task;

pub struct DynNotifier(Box<dyn ExecutorNotified + Send>);

impl ExecutorNotified for DynNotifier {
    fn request_cancel(&mut self) {
        self.0.request_cancel()
    }
}

impl<UnderlyingNotifier: ExecutorNotified + Send> SomeExecutor for Box<dyn SomeExecutor<ExecutorNotifier = UnderlyingNotifier>> {
    type ExecutorNotifier = DynNotifier;

    fn spawn<F: Future + Send + 'static, Notifier: ObserverNotified<F::Output> + Send>(&mut self, task: Task<F, Notifier>) -> impl Observer<Value=F::Output>
    where
        Self: Sized,
        F::Output: Send + Unpin
    {
        let underlying = self.as_mut();
        let objsafe = task.into_objsafe();
        let observer = underlying.spawn_objsafe(objsafe);
        //write in the type again
        let downcasted: DowncastObserver<_,F::Output> =  DowncastObserver::new(observer);
        downcasted
    }

    fn spawn_async<'s, F: Future + Send + 'static, Notifier: ObserverNotified<F::Output> + Send>(&'s mut self, task: Task<F, Notifier>) -> impl Future<Output=impl Observer<Value=F::Output>> + Send + 's
    where
        Self: Sized,
        F::Output: Send + Unpin
    {



        async {
            let underlying = self.as_mut();
            let objsafe = task.into_objsafe();
            let observer = underlying.spawn_objsafe(objsafe);
            //write in the type again
            let downcasted: DowncastObserver<_,F::Output> =  DowncastObserver::new(observer);
            downcasted
        }
    }

    fn spawn_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any + 'static + Send>> + 'static + Send>>, Box<dyn ObserverNotified<dyn Any + Send> + Send>>) -> Box<dyn Observer<Value=Box<dyn Any + Send>>> {
        let (spawn, observer) = task.spawn_objsafe(self);
        todo!("actually spawn");

        Box::new(observer)
    }

    fn spawn_objsafe_async(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any + 'static + Send>> + 'static + Send>>, Box<dyn ObserverNotified<dyn Any + Send> + Send>>) -> Box<dyn Future<Output=Box<dyn Observer<Value=Box<dyn Any + Send>>>>> {
        todo!() 
    }

    fn clone_box(&self) -> Box<DynExecutor> {
        self.as_ref().clone_box()
    }

    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        let underlying = self.as_mut().executor_notifier();
        underlying.map(|u| DynNotifier(Box::new(u)))
    }
}

#[cfg(test)] mod tests {
    use crate::{SomeExecutor,Infallible};
    #[test] fn test_dyn_executor() {
        //mt2-697
        fn just_compile() {
            let _ty: Box<dyn SomeExecutor<ExecutorNotifier = Infallible>> = todo!();
            fn expect_executor<E: SomeExecutor>(e: E) {}
            expect_executor(_ty);
        }
    }
}