// SPDX-License-Identifier: MIT OR Apache-2.0

/*!
This local executor is in use when no other local executors are registered.

It is intentionally the simplest idea possible, but it ensures a compliant local executor is always available.

> Cut my tasks into pieces, this is my last resort!
> Async handling, no tokio, don't give a fuck if performance is bleeding
> this is my last resort

*/

use crate::observer::{ExecutorNotified, FinishedObservation, Observer, ObserverNotified};
use crate::task::Task;
use crate::{LocalExecutorExt, SomeLocalExecutor};
use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, RawWaker, RawWakerVTable};

pub(crate) struct LocalLastResortExecutor;

impl LocalLastResortExecutor {
    pub fn new() -> Self {
        LocalLastResortExecutor
    }
}

fn print_warning() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        eprintln!(
            "some_executor::LocalLastResortExecutor is in use. This is not intended for production code; investigate ways to use a production-quality executor."
        );
    }
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::console::log_1(&"some_executor::LocalLastResortExecutor is in use. This is not intended for production code; investigate ways to use a production-quality executor.".into());
    }
}

// Same pattern as the global last resort executor
const SLEEPING: u8 = 0;
const LISTENING: u8 = 1;
const WAKEPLS: u8 = 2;

struct Shared {
    condvar: Condvar,
    mutex: Mutex<bool>,
    inline_notify: AtomicU8,
}

struct Waker {
    shared: Arc<Shared>,
}

static WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    |data| {
        let waker = unsafe { Arc::from_raw(data as *const Waker) };
        let w2 = waker.clone();
        std::mem::forget(waker);
        RawWaker::new(Arc::into_raw(w2) as *const (), &WAKER_VTABLE)
    },
    |data| {
        let waker = unsafe { Arc::from_raw(data as *const Waker) };
        let old = waker.shared.inline_notify.swap(WAKEPLS, Ordering::Relaxed);
        if old == SLEEPING {
            waker.shared.condvar.notify_one();
        }
        drop(waker);
    },
    |data| {
        let waker = unsafe { Arc::from_raw(data as *const Waker) };
        let old = waker.shared.inline_notify.swap(WAKEPLS, Ordering::Relaxed);
        if old == SLEEPING {
            waker.shared.condvar.notify_one();
        }
        std::mem::forget(waker);
    },
    |data| {
        let waker = unsafe { Arc::from_raw(data as *const Waker) };
        drop(waker);
    },
);

impl Waker {
    fn into_core_waker(self) -> core::task::Waker {
        let data = Arc::into_raw(Arc::new(self));
        unsafe { core::task::Waker::from_raw(RawWaker::new(data as *const (), &WAKER_VTABLE)) }
    }
}

// Helper to run a SpawnedLocalTask to completion using condvar/mutex
fn run_local_task<F, N>(mut spawned: crate::task::SpawnedLocalTask<F, N, LocalLastResortExecutor>)
where
    F: Future,
    N: ObserverNotified<F::Output>,
{
    let shared = Arc::new(Shared {
        condvar: Condvar::new(),
        mutex: Mutex::new(false),
        inline_notify: AtomicU8::new(SLEEPING),
    });
    let waker = Waker {
        shared: shared.clone(),
    }
    .into_core_waker();
    let mut context = Context::from_waker(&waker);
    let mut executor = LocalLastResortExecutor::new();
    let mut pinned = unsafe { Pin::new_unchecked(&mut spawned) };

    loop {
        let mut _guard = shared.mutex.lock().expect("Mutex poisoned");
        // Eagerly poll
        shared.inline_notify.store(LISTENING, Ordering::Relaxed);
        let r = pinned.as_mut().poll(&mut context, &mut executor, None);
        match r {
            Poll::Ready(()) => {
                return;
            }
            Poll::Pending => {
                let old = shared.inline_notify.swap(SLEEPING, Ordering::Relaxed);
                if old == WAKEPLS {
                    // Release lock anyway
                    drop(_guard);
                    continue; // Poll eagerly
                } else {
                    _guard = shared
                        .condvar
                        .wait_while(_guard, |_| {
                            shared.inline_notify.load(Ordering::Relaxed) != WAKEPLS
                        })
                        .expect("Condvar poisoned");
                }
            }
        }
    }
}

impl<'a> SomeLocalExecutor<'a> for LocalLastResortExecutor {
    type ExecutorNotifier = Box<dyn ExecutorNotified>;

    fn spawn_local<F: Future + 'a, Notifier: ObserverNotified<F::Output>>(
        &mut self,
        task: Task<F, Notifier>,
    ) -> impl Observer<Value = F::Output>
    where
        Self: Sized,
        F::Output: Unpin + 'static,
    {
        print_warning();

        let (spawned, observer) = task.spawn_local(self);

        // We need to handle lifetime issues here. Since this is a last resort executor,
        // we'll run the task synchronously on the current thread.
        run_local_task(spawned);

        observer
    }

    fn spawn_local_async<F: Future + 'a, Notifier: ObserverNotified<F::Output>>(
        &mut self,
        task: Task<F, Notifier>,
    ) -> impl Future<Output = impl Observer<Value = F::Output>>
    where
        Self: Sized,
        F::Output: 'static + Unpin,
    {
        print_warning();

        let (spawned, observer) = task.spawn_local(self);

        // Run the task synchronously and return a ready future
        run_local_task(spawned);

        std::future::ready(observer)
    }

    fn spawn_local_objsafe(
        &mut self,
        task: Task<
            Pin<Box<dyn Future<Output = Box<dyn Any>>>>,
            Box<dyn ObserverNotified<(dyn Any + 'static)>>,
        >,
    ) -> Box<dyn Observer<Value = Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>> {
        print_warning();

        let (spawned, observer) = task.spawn_local_objsafe(self);

        run_local_task(spawned);

        Box::new(observer)
    }

    fn spawn_local_objsafe_async<'s>(
        &'s mut self,
        task: Task<
            Pin<Box<dyn Future<Output = Box<dyn Any>>>>,
            Box<dyn ObserverNotified<(dyn Any + 'static)>>,
        >,
    ) -> Box<
        dyn Future<
                Output = Box<
                    dyn Observer<Value = Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>,
                >,
            > + 's,
    > {
        print_warning();

        let observer = self.spawn_local_objsafe(task);
        Box::new(std::future::ready(observer))
    }

    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        None
    }
}

impl LocalExecutorExt<'static> for LocalLastResortExecutor {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observer::Observer;
    use crate::task::{Configuration, Task};
    use std::future::Future;
    use std::pin::Pin;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::task::{Context, Poll};

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_basic_spawn_local() {
        let mut executor = LocalLastResortExecutor::new();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let task = Task::without_notifications(
            "test-task".to_string(),
            Configuration::default(),
            async move {
                counter_clone.fetch_add(1, Ordering::Relaxed);
                42
            },
        );

        let observer = executor.spawn_local(task);
        // Since our executor runs synchronously, check observe() result
        match observer.observe() {
            crate::observer::Observation::Ready(value) => {
                assert_eq!(value, 42);
            }
            _ => panic!("Task should have completed immediately"),
        }
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_non_send_future() {
        let mut executor = LocalLastResortExecutor::new();

        // Create a non-Send type (Rc cannot be sent across threads)
        let non_send_data = Rc::new(42);
        let data_clone = non_send_data.clone();

        let task = Task::without_notifications(
            "non-send-task".to_string(),
            Configuration::default(),
            async move {
                let _captured = data_clone; // This makes the future !Send
                "completed"
            },
        );

        let observer = executor.spawn_local(task);
        match observer.observe() {
            crate::observer::Observation::Ready(value) => {
                assert_eq!(value, "completed");
            }
            _ => panic!("Task should have completed immediately"),
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_spawn_local_async() {
        let mut executor = LocalLastResortExecutor::new();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let task = Task::without_notifications(
            "async-task".to_string(),
            Configuration::default(),
            async move {
                counter_clone.fetch_add(10, Ordering::Relaxed);
                100
            },
        );

        // Use a simple runtime to test the async spawn
        let future = executor.spawn_local_async(task);
        let mut pinned = std::pin::pin!(future);

        // For the test, we'll use a simple polling approach
        let waker = Arc::new(DummyWaker).into();
        let mut context = std::task::Context::from_waker(&waker);

        match pinned.as_mut().poll(&mut context) {
            std::task::Poll::Ready(obs) => match obs.observe() {
                crate::observer::Observation::Ready(value) => {
                    assert_eq!(value, 100);
                    assert_eq!(counter.load(Ordering::Relaxed), 10);
                }
                _ => panic!("Task should have completed immediately"),
            },
            std::task::Poll::Pending => panic!("Task should complete immediately"),
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_spawn_local_objsafe() {
        let mut executor = LocalLastResortExecutor::new();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let future: Pin<Box<dyn Future<Output = Box<dyn Any>>>> = Box::pin(async move {
            counter_clone.fetch_add(5, Ordering::Relaxed);
            Box::new(50i32) as Box<dyn Any>
        });

        let task = Task::without_notifications(
            "objsafe-task".to_string(),
            Configuration::default(),
            future,
        );

        let observer = executor.spawn_local_objsafe(task.into_objsafe_local());
        match observer.observe() {
            crate::observer::Observation::Ready(result) => {
                // The future returns Box::new(50i32) as Box<dyn Any>
                // into_objsafe_local wraps this again, so we get Box<dyn Any> containing Box<dyn Any> containing i32
                let inner_box = result
                    .downcast::<Box<dyn Any>>()
                    .expect("Should be Box<dyn Any>");
                let value = inner_box.downcast::<i32>().expect("Should be i32");
                assert_eq!(*value, 50);
                assert_eq!(counter.load(Ordering::Relaxed), 5);
            }
            _ => panic!("Task should have completed immediately"),
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_executor_notifier() {
        let mut executor = LocalLastResortExecutor::new();
        assert!(executor.executor_notifier().is_none());
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_thread_local_executor_integration() {
        use crate::thread_executor::thread_local_executor;

        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        // Test that thread_local_executor provides a working executor
        thread_local_executor(|executor| {
            let task = Task::without_notifications(
                "thread-local-test".to_string(),
                Configuration::default(),
                async move {
                    counter_clone.fetch_add(100, Ordering::Relaxed);
                    "thread-local-result"
                },
            );

            let observer = executor.spawn_local_objsafe(task.into_objsafe_local());
            match observer.observe() {
                crate::observer::Observation::Ready(result) => {
                    let value = result.downcast::<&str>().expect("Should be &str");
                    assert_eq!(*value, "thread-local-result");
                }
                _ => panic!("Task should have completed immediately"),
            }
        });

        assert_eq!(counter.load(Ordering::Relaxed), 100);
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_delayed_waking() {
        let mut executor = LocalLastResortExecutor::new();

        // Create a future that needs to be polled 3 times before completion
        let delayed_future = DelayedFuture::new(3, 99);
        let poll_counter = delayed_future.poll_count.clone();

        let task = Task::without_notifications(
            "delayed-task".to_string(),
            Configuration::default(),
            delayed_future,
        );

        let observer = executor.spawn_local(task);

        // Verify the task completed successfully
        match observer.observe() {
            crate::observer::Observation::Ready(value) => {
                assert_eq!(value, 99);
                // Verify it was polled exactly 4 times (3 pending + 1 ready)
                assert_eq!(poll_counter.load(Ordering::Relaxed), 4);
            }
            _ => panic!("Task should have completed successfully"),
        }
    }

    // Helper struct for async test
    struct DummyWaker;

    impl std::task::Wake for DummyWaker {
        fn wake(self: Arc<Self>) {}
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
                #[cfg(not(target_arch = "wasm32"))]
                {
                    std::thread::spawn(move || {
                        // Give time for the executor to enter the condvar wait
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        for _ in 0..5 {
                            waker.wake_by_ref();
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                    });
                }
                #[cfg(target_arch = "wasm32")]
                {
                    // For WASM, we'll use a simple immediate wake since we can't use std::thread
                    waker.wake();
                }
            }

            Poll::Pending
        }
    }
}
