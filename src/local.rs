use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use crate::observer::{ExecutorNotified, Observer, ObserverNotified};
use crate::{DynONotifier, SomeLocalExecutor};
use crate::task::Task;

struct SomeLocalExecutorErasingNotifier<'underlying, UnderlyingExecutor: SomeLocalExecutor + ?Sized> {
    executor: &'underlying mut UnderlyingExecutor,
}

impl<'executor, UnderlyingExecutor: SomeLocalExecutor> SomeLocalExecutor for SomeLocalExecutorErasingNotifier<'executor, UnderlyingExecutor> {
    type ExecutorNotifier = Box<dyn ExecutorNotified + 'executor>;

    fn spawn_local<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F, Notifier>) -> Observer<F::Output, Self::ExecutorNotifier>
    where
        Self: Sized,
        <F as Future>::Output: Unpin,
    /*I am a little uncertain as to whether this is really required */
    {
        let o = self.executor.spawn_local(task);
        o.into_boxed_notifier()
    }

    fn spawn_local_async<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F, Notifier>) -> impl Future<Output=Observer<F::Output, Self::ExecutorNotifier>>
    where
        Self: Sized,
    {
        async {
            let o = self.executor.spawn_local_async(task).await;
            o.into_boxed_notifier()
        }
    }


    fn spawn_local_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Observer<Box<dyn Any>, Box<dyn ExecutorNotified>> {
        let o = self.executor.spawn_local_objsafe(task);
        o.into_boxed_notifier()
    }

    fn spawn_local_objsafe_async<'e>(&'e mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<DynONotifier>>) -> Box<dyn Future<Output=Observer<Box<dyn Any>, Box<dyn ExecutorNotified>>> + 'e> {
        Box::new(async {
            let f = self.executor.spawn_local_objsafe_async(task);
            let f = Box::into_pin(f);
            let o = f.await;
            o.into_boxed_notifier()
        })
    }

    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        self.executor.executor_notifier().map(|x| Box::new(x) as Self::ExecutorNotifier)
    }
}
