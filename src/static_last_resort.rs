// SPDX-License-Identifier: MIT OR Apache-2.0

/*!
This static executor is in use when no other static executors are registered.

It is intentionally the simplest idea possible, but it ensures a compliant static executor is always available.

> Cut my tasks into pieces, this is my last resort!
> Async handling, no tokio, don't give a fuck if performance is bleeding
> this is my last resort

*/

use crate::observer::{ExecutorNotified, Observer, ObserverNotified};
use crate::task::Task;
use crate::{
    BoxedStaticObserver, BoxedStaticObserverFuture, DynStaticExecutor, ObjSafeStaticTask,
    SomeStaticExecutor, StaticExecutorExt,
};
use std::future::Future;

#[derive(Clone, Debug)]
pub(crate) struct StaticLastResortExecutor;

impl StaticLastResortExecutor {
    pub fn new() -> Self {
        StaticLastResortExecutor
    }
}

fn print_warning() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        eprintln!(
            "some_executor::StaticLastResortExecutor is in use. This is not intended for production code; investigate ways to use a production-quality executor."
        );
    }
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::console::log_1(&"some_executor::StaticLastResortExecutor is in use. This is not intended for production code; investigate ways to use a production-quality executor.".into());
    }
}

impl SomeStaticExecutor for StaticLastResortExecutor {
    type ExecutorNotifier = Box<dyn ExecutorNotified>;

    fn spawn_static<F: Future + 'static, Notifier: ObserverNotified<F::Output>>(
        &mut self,
        task: Task<F, Notifier>,
    ) -> impl Observer<Value = F::Output>
    where
        Self: Sized,
        F::Output: Unpin + 'static,
    {
        print_warning();

        let (spawned, observer) = task.spawn_static(self);

        // We need to handle lifetime issues here. Since this is a last resort executor,
        // we'll run the task synchronously on the current thread.
        crate::sys::run_static_task(spawned);

        observer
    }

    fn spawn_static_async<F: Future + 'static, Notifier: ObserverNotified<F::Output>>(
        &mut self,
        task: Task<F, Notifier>,
    ) -> impl Future<Output = impl Observer<Value = F::Output>>
    where
        Self: Sized,
        F::Output: 'static + Unpin,
    {
        print_warning();

        let (spawned, observer) = task.spawn_static(self);

        // Run the task synchronously and return a ready future
        crate::sys::run_static_task(spawned);

        std::future::ready(observer)
    }

    fn spawn_static_objsafe(&mut self, task: ObjSafeStaticTask) -> BoxedStaticObserver {
        print_warning();

        let (spawned, observer) = task.spawn_static_objsafe(self);

        crate::sys::run_static_task(spawned);

        Box::new(observer)
    }

    fn spawn_static_objsafe_async<'s>(
        &'s mut self,
        task: ObjSafeStaticTask,
    ) -> BoxedStaticObserverFuture<'s> {
        print_warning();

        let observer = self.spawn_static_objsafe(task);
        Box::new(std::future::ready(observer))
    }

    fn clone_box(&self) -> Box<DynStaticExecutor> {
        Box::new(self.clone())
    }

    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        None
    }
}

impl StaticExecutorExt for StaticLastResortExecutor {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observer::{FinishedObservation, Observer};
    use crate::task::{Configuration, Task};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::task::{Context, Poll};

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_basic_spawn_static() {
        let (s, r) = std::sync::mpsc::channel();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        crate::sys::thread::spawn(move || {
            let mut executor = StaticLastResortExecutor::new();

            let task = Task::without_notifications(
                "test-task".to_string(),
                Configuration::default(),
                async move {
                    counter_clone.fetch_add(1, Ordering::Relaxed);
                    42
                },
            );

            let observer = executor.spawn_static(task);

            // Create and spawn a second task to do the observation
            let observation_task = Task::without_notifications(
                "test-task-observer".to_string(),
                Configuration::default(),
                async move {
                    match observer.await {
                        FinishedObservation::Ready(value) => {
                            assert_eq!(value, 42);
                            s.send(Ok(counter.load(Ordering::Relaxed))).unwrap();
                        }
                        _ => {
                            s.send(Err("Task did not complete successfully")).unwrap();
                        }
                    }
                },
            );
            executor.spawn_static(observation_task).detach();

            // Block on receiver
            match r.recv() {
                Ok(Ok(counter_value)) => {
                    assert_eq!(counter_value, 1);
                }
                Ok(Err(e)) => panic!("Task failed: {}", e),
                Err(e) => panic!("Observation failed: {}", e),
            }
        });
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_static_future() {
        let (s, r) = std::sync::mpsc::channel();

        crate::sys::thread::spawn(move || {
            let mut executor = StaticLastResortExecutor::new();

            // Create a static future - using String instead of Rc since static futures need to be Send-like
            let static_data = Arc::new(42);
            let data_clone = static_data.clone();

            let task = Task::without_notifications(
                "static-task".to_string(),
                Configuration::default(),
                async move {
                    let _captured = data_clone; // This makes the future 'static
                    "completed"
                },
            );

            let observer = executor.spawn_static(task);

            // Create and spawn a second task to do the observation
            let observation_task = Task::without_notifications(
                "static-task-observer".to_string(),
                Configuration::default(),
                async move {
                    match observer.await {
                        FinishedObservation::Ready(value) => {
                            assert_eq!(value, "completed");
                            s.send(Ok(())).unwrap();
                        }
                        _ => {
                            s.send(Err("Task did not complete successfully")).unwrap();
                        }
                    }
                },
            );
            executor.spawn_static(observation_task).detach();

            // Block on receiver
            match r.recv() {
                Ok(_) => {}
                Err(e) => panic!("Observation failed: {}", e),
            }
        });
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_spawn_static_async() {
        let (s, r) = std::sync::mpsc::channel();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();
        let counter_clone2 = counter.clone();

        crate::sys::thread::spawn(move || {
            let mut executor = StaticLastResortExecutor::new();

            // Create a task that tests spawn_static_async
            let test_task = Task::without_notifications(
                "test-spawn-async".to_string(),
                Configuration::default(),
                async move {
                    let mut executor2 = StaticLastResortExecutor::new();

                    let inner_task = Task::without_notifications(
                        "async-task".to_string(),
                        Configuration::default(),
                        async move {
                            counter_clone.fetch_add(10, Ordering::Relaxed);
                            100
                        },
                    );

                    // Test spawn_static_async
                    let observer = executor2.spawn_static_async(inner_task).await;
                    match observer.await {
                        FinishedObservation::Ready(value) => {
                            assert_eq!(value, 100);
                            s.send(Ok(counter_clone2.load(Ordering::Relaxed))).unwrap();
                        }
                        _ => {
                            s.send(Err("Task did not complete successfully")).unwrap();
                        }
                    }
                },
            );
            executor.spawn_static(test_task).detach();

            // Block on receiver
            match r.recv() {
                Ok(Ok(counter_value)) => {
                    assert_eq!(counter_value, 10);
                }
                Ok(Err(e)) => panic!("Task failed: {}", e),
                Err(e) => panic!("Observation failed: {}", e),
            }
        });
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_spawn_static_objsafe() {
        let (s, r) = std::sync::mpsc::channel();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        crate::sys::thread::spawn(move || {
            let mut executor = StaticLastResortExecutor::new();

            let future: Pin<Box<dyn Future<Output = Box<dyn std::any::Any>> + 'static>> =
                Box::pin(async move {
                    counter_clone.fetch_add(5, Ordering::Relaxed);
                    Box::new(50i32) as Box<dyn std::any::Any>
                });

            let task = Task::without_notifications(
                "objsafe-task".to_string(),
                Configuration::default(),
                future,
            );

            let observer = executor.spawn_static_objsafe(task.into_objsafe_static());

            // Create and spawn a second task to do the observation
            let observation_task = Task::without_notifications(
                "objsafe-task-observer".to_string(),
                Configuration::default(),
                async move {
                    match observer.await {
                        FinishedObservation::Ready(result) => {
                            // The future returns Box::new(50i32) as Box<dyn Any>
                            // into_objsafe_static wraps this again, so we get Box<dyn Any> containing Box<dyn Any> containing i32
                            let inner_box = result
                                .downcast::<Box<dyn std::any::Any>>()
                                .expect("Should be Box<dyn Any>");
                            let value = inner_box.downcast::<i32>().expect("Should be i32");
                            assert_eq!(*value, 50);
                            s.send(Ok(counter.load(Ordering::Relaxed))).unwrap();
                        }
                        _ => {
                            s.send(Err("Task did not complete successfully")).unwrap();
                        }
                    }
                },
            );
            executor.spawn_static(observation_task).detach();

            // Block on receiver
            match r.recv() {
                Ok(Ok(counter_value)) => {
                    assert_eq!(counter_value, 5);
                }
                Ok(Err(e)) => panic!("Task failed: {}", e),
                Err(e) => panic!("Observation failed: {}", e),
            }
        });
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_executor_notifier() {
        let mut executor = StaticLastResortExecutor::new();
        assert!(executor.executor_notifier().is_none());
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_delayed_waking() {
        let (s, r) = std::sync::mpsc::channel();
        let delayed_future = DelayedFuture::new(3, 99);
        let _poll_counter = delayed_future.poll_count.clone();

        crate::sys::thread::spawn(move || {
            let mut executor = StaticLastResortExecutor::new();

            // Create a future that needs to be polled 3 times before completion
            let task = Task::without_notifications(
                "delayed-task".to_string(),
                Configuration::default(),
                delayed_future,
            );

            let observer = executor.spawn_static(task);
            //create and spawn a second task to do the observation
            let observation_task = Task::without_notifications(
                "delayed-task-observer".to_string(),
                Configuration::default(),
                async move {
                    // Verify the task completed successfully
                    match observer.await {
                        FinishedObservation::Ready(value) => {
                            assert_eq!(value, 99);
                            s.send(Ok(())).unwrap();
                        }
                        _ => {
                            s.send(Err("Task did not complete successfully")).unwrap();
                        }
                    }
                },
            );
            executor.spawn_static(observation_task).detach();
            //block on receiver
            match r.recv() {
                Ok(_) => println!("Delayed task completed successfully"),
                Err(e) => panic!("Observation failed: {}", e),
            }
        });
    }

    // Custom future that polls N times before completion
    struct DelayedFuture {
        poll_count: Arc<AtomicU32>,
        max_polls: u32,
        result_value: i32,
        waker_spawned: Arc<AtomicU32>,
    }

    impl DelayedFuture {
        fn new(max_polls: u32, result_value: i32) -> Self {
            Self {
                poll_count: Arc::new(AtomicU32::new(0)),
                max_polls,
                result_value,
                waker_spawned: Arc::new(AtomicU32::new(0)),
            }
        }
    }

    impl Future for DelayedFuture {
        type Output = i32;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let current_count = self.poll_count.fetch_add(1, Ordering::Relaxed);

            if current_count >= self.max_polls {
                return Poll::Ready(self.result_value);
            }

            // Only spawn the waking thread once
            if self
                .waker_spawned
                .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                let waker = cx.waker().clone();
                crate::sys::thread::spawn(move || {
                    // Give time for the executor to enter the condvar wait
                    crate::sys::thread::sleep(std::time::Duration::from_millis(100));
                    for _ in 0..5 {
                        waker.wake_by_ref();
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                });
            }

            Poll::Pending
        }
    }
}
