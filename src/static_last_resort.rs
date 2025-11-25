// SPDX-License-Identifier: MIT OR Apache-2.0

/*!
A fallback static executor that provides a guaranteed async runtime when no other executors are available.

This module implements `StaticLastResortExecutor`, a minimal but compliant static executor that serves
as the ultimate fallback in the `some_executor` hierarchy. When no production-quality static executor
is configured, this executor ensures that static async code can still run, albeit with significant
performance limitations.

# Design Philosophy

The `StaticLastResortExecutor` is intentionally designed to be the simplest possible executor that
still correctly implements the `SomeStaticExecutor` trait. It prioritizes correctness and availability
over performance, making it suitable only for development, testing, or emergency fallback scenarios.

# Implementation Strategy

This executor uses a synchronous, blocking approach:
- Tasks are executed immediately and synchronously on the calling thread
- No actual concurrent execution occurs - everything runs sequentially
- Platform-specific synchronization primitives (condvar/mutex on std, busy-waiting on WASM)
- Emits warnings when used to alert developers about the performance implications

# When This Executor Is Used

The static last resort executor activates when:
1. No global static executor has been configured via `set_static_executor()`
2. No thread-local static executor is available
3. Code attempts to spawn a static future without proper executor setup

# Performance Characteristics

**Warning**: This executor is NOT intended for production use. It has severe limitations:
- **No concurrency**: All tasks run synchronously, blocking the calling thread
- **Poor scalability**: Each spawn blocks until the task completes
- **High overhead**: Uses heavyweight synchronization primitives for simple polling
- **No work stealing**: Tasks cannot be distributed across threads
- **No I/O optimization**: Blocking I/O will block the entire executor

# Platform Differences

## Standard Platforms (non-WASM)
Uses `std::sync::Condvar` and `std::sync::Mutex` for wake notifications, providing
efficient blocking waits when futures return `Poll::Pending`.

## WASM32
Falls back to busy-waiting or platform-specific primitives since standard blocking
primitives are not available in browser environments.

# Example

```
// This example shows when the last resort executor would be used.
// In production, you should configure a proper executor instead.

use some_executor::thread_executor::thread_static_executor;

// When no executor is set, the last resort executor is used automatically
thread_static_executor(|executor| {
    // This executor will be the last resort if none was configured
    // It prints a warning to stderr when used

    // The StaticLastResortExecutor is an internal implementation detail
    // that gets invoked automatically as a fallback
    println!("Using executor: {:?}", std::any::type_name_of_val(executor));
});
```

# Alternatives

For production use, consider these alternatives:
- Configure a global static executor at application startup
- Use thread-local executors for isolated execution contexts
- Integrate with established runtimes like Tokio or async-std

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

/// A minimal static executor that runs tasks synchronously as a last resort fallback.
///
/// This executor provides a compliant implementation of [`SomeStaticExecutor`] but with
/// severe performance limitations. It's designed to ensure that async code can always
/// run, even when no production executor is configured.
///
/// # Warning
///
/// This executor is NOT suitable for production use. It executes all tasks synchronously
/// on the calling thread, providing no actual concurrency. A warning is printed to stderr
/// (or console on WASM) whenever this executor is used.
///
/// # Implementation Details
///
/// When a task is spawned:
/// 1. The task is immediately executed synchronously using platform-specific primitives
/// 2. The executor blocks until the task completes
/// 3. An observer handle is returned for compatibility, but the task has already finished
///
/// # Thread Safety
///
/// `StaticLastResortExecutor` is `Clone` and can be safely shared between threads.
/// However, each spawn operation blocks the calling thread until completion, so
/// concurrent spawns from different threads will effectively serialize.
///
/// # Memory Safety
///
/// The executor properly handles task lifetimes and waker management through the
/// platform-specific `run_static_task` implementation, ensuring no use-after-free
/// or memory leaks occur

#[derive(Clone, Debug)]
pub(crate) struct StaticLastResortExecutor;

impl StaticLastResortExecutor {
    /// Creates a new instance of the static last resort executor.
    ///
    /// This executor should only be used when no other static executor is available.
    /// It will print a warning when first used to alert developers about the
    /// performance implications.
    pub fn new() -> Self {
        StaticLastResortExecutor
    }
}

/// Prints a warning message indicating that the last resort executor is being used.
///
/// This function alerts developers that they are using a fallback executor with poor
/// performance characteristics. The warning is printed to:
/// - `stderr` on standard platforms via `eprintln!`
/// - Browser console on WASM32 targets via `web_sys::console::log_1`
///
/// The warning is printed each time a task is spawned to ensure developers are aware
/// of the performance implications.
///
/// # Note
///
/// In production builds, consider configuring a proper executor to avoid these warnings
/// and achieve better performance.
fn print_warning() {
    const MESSAGE: &str = "some_executor::StaticLastResortExecutor is in use. This is not intended for production code; investigate ways to use a production-quality executor. Set SOME_EXECUTOR_BUILTIN_SHOULD_PANIC=1 to panic instead.";

    let should_panic = std::env::var("SOME_EXECUTOR_BUILTIN_SHOULD_PANIC")
        .map(|v| {
            let v = v.to_lowercase();
            v == "1" || v == "true" || v == "yes"
        })
        .unwrap_or(false);

    if should_panic {
        panic!("{}", MESSAGE);
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        eprintln!("{}", MESSAGE);
    }
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::console::log_1(&MESSAGE.into());
    }
}

impl SomeStaticExecutor for StaticLastResortExecutor {
    type ExecutorNotifier = Box<dyn ExecutorNotified>;

    /// Spawns a static future and executes it synchronously.
    ///
    /// This method immediately executes the provided task to completion on the current thread.
    /// Despite appearing asynchronous, the execution is entirely synchronous - the task
    /// completes before this method returns.
    ///
    /// # Parameters
    ///
    /// - `task`: The task containing a `'static` future to execute
    ///
    /// # Returns
    ///
    /// An [`Observer`] handle for the completed task. Since execution is synchronous,
    /// the task has already finished when this method returns.
    ///
    /// # Performance Warning
    ///
    /// This method blocks the calling thread until the task completes. It prints a warning
    /// to alert developers about the performance implications.
    ///
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

    /// Spawns a static future "asynchronously" but actually executes it synchronously.
    ///
    /// Despite returning a `Future`, this method executes the task synchronously before
    /// returning. The returned future is already resolved with the observer handle.
    ///
    /// # Parameters
    ///
    /// - `task`: The task containing a `'static` future to execute
    ///
    /// # Returns
    ///
    /// A future that immediately resolves to an [`Observer`] handle. The future is always
    /// ready since the task executes synchronously.
    ///
    /// # Performance Warning
    ///
    /// Like `spawn_static`, this blocks the calling thread until task completion.
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

    /// Spawns an object-safe static task and executes it synchronously.
    ///
    /// This method handles type-erased tasks, allowing the executor to work with
    /// dynamic dispatch scenarios. Like other spawn methods, execution is synchronous.
    ///
    /// # Parameters
    ///
    /// - `task`: An object-safe task wrapper containing a type-erased future
    ///
    /// # Returns
    ///
    /// A boxed observer for the completed task.
    ///
    /// # Performance Warning
    ///
    /// Blocks the calling thread until completion and prints a warning.
    fn spawn_static_objsafe(&mut self, task: ObjSafeStaticTask) -> BoxedStaticObserver {
        print_warning();

        let (spawned, observer) = task.spawn_static_objsafe(self);

        crate::sys::run_static_task(spawned);

        Box::new(observer)
    }

    /// Spawns an object-safe static task "asynchronously" but executes it synchronously.
    ///
    /// Similar to `spawn_static_async`, this returns an immediately ready future
    /// containing the observer, as the task executes synchronously.
    ///
    /// # Parameters
    ///
    /// - `task`: An object-safe task wrapper containing a type-erased future
    ///
    /// # Returns
    ///
    /// A boxed future that immediately resolves to a boxed observer.
    ///
    /// # Performance Warning
    ///
    /// Blocks the calling thread until completion and prints a warning.
    fn spawn_static_objsafe_async<'s>(
        &'s mut self,
        task: ObjSafeStaticTask,
    ) -> BoxedStaticObserverFuture<'s> {
        print_warning();

        let observer = self.spawn_static_objsafe(task);
        Box::new(std::future::ready(observer))
    }

    /// Creates a boxed clone of this executor for dynamic dispatch.
    ///
    /// This enables the executor to be used in contexts requiring trait objects.
    ///
    /// # Returns
    ///
    /// A boxed clone of the executor.
    fn clone_box(&self) -> Box<DynStaticExecutor> {
        Box::new(self.clone())
    }

    /// Returns the executor's notifier, if available.
    ///
    /// The last resort executor does not provide a notifier since it executes
    /// tasks synchronously and has no event loop to notify.
    ///
    /// # Returns
    ///
    /// Always returns `None` as this executor has no notification mechanism.
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

    // === ISOLATION TESTS FOR DEBUGGING ===

    /// Test 0: Does anything work at all?
    #[test_executors::async_test]
    async fn isolation_test_sanity() {
        // This should pass immediately with no dependencies
        assert_eq!(1 + 1, 2);
    }

    /// Test 1: Does thread::spawn work at all? (async, no blocking)
    #[test_executors::async_test]
    async fn isolation_test_thread_spawn() {
        use std::sync::atomic::AtomicBool;
        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = flag.clone();
        let (done_tx, done_rx) = r#continue::continuation::<()>();

        crate::sys::thread::spawn(move || {
            flag_clone.store(true, Ordering::SeqCst);
            done_tx.send(());
        });

        // Await completion without blocking
        done_rx.await;
        assert!(flag.load(Ordering::SeqCst), "Thread should have run");
    }

    /// Test 2: Does mpsc channel work (same thread, no spawn)?
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn isolation_test_mpsc_same_thread() {
        let (s, r) = std::sync::mpsc::channel();
        s.send(42).unwrap();
        assert_eq!(r.recv().unwrap(), 42);
    }

    /// Test 3: Does executor poll tasks (no threading)?
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn isolation_test_executor_no_thread() {
        let mut executor = StaticLastResortExecutor::new();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let task = Task::without_notifications(
            "simple-task".to_string(),
            Configuration::default(),
            async move {
                counter_clone.fetch_add(1, Ordering::Relaxed);
                42
            },
        );

        let observer = executor.spawn_static(task);

        // Poll until complete by awaiting in another task
        let result_task = Task::without_notifications(
            "result-task".to_string(),
            Configuration::default(),
            async move { observer.await },
        );
        let result_observer = executor.spawn_static(result_task);

        // Spin until done (no blocking)
        loop {
            match result_observer.observe() {
                crate::observer::Observation::Ready(finished) => match finished {
                    FinishedObservation::Ready(_) => break,
                    other => panic!("Unexpected: {:?}", other),
                },
                crate::observer::Observation::Pending => {
                    // Keep spinning
                    std::hint::spin_loop();
                }
                crate::observer::Observation::Done | crate::observer::Observation::Cancelled => {
                    panic!("Task was done or cancelled unexpectedly");
                }
            }
        }

        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    /// Test 4: Does executor work inside a spawned thread?
    #[cfg(not(target_arch = "wasm32"))]
    #[test_executors::async_test]
    async fn isolation_test_executor_in_thread() {
        #[cfg(target_arch = "wasm32")]
        use wasm_bindgen::JsValue;

        #[cfg(target_arch = "wasm32")]
        macro_rules! log {
            ($($arg:tt)*) => {
                web_sys::console::log_1(&JsValue::from_str(&format!($($arg)*)));
            };
        }
        #[cfg(not(target_arch = "wasm32"))]
        macro_rules! log {
            ($($arg:tt)*) => {
                eprintln!($($arg)*);
            };
        }

        log!("TEST: Starting isolation_test_executor_in_thread");

        let (done_tx, done_rx) = r#continue::continuation::<u32>();

        log!("TEST: About to spawn thread");

        crate::sys::thread::spawn(move || {
            // Note: logging from worker may not work, but we try anyway
            #[cfg(target_arch = "wasm32")]
            web_sys::console::log_1(&wasm_bindgen::JsValue::from_str("WORKER: Thread started"));

            let mut executor = StaticLastResortExecutor::new();

            #[cfg(target_arch = "wasm32")]
            web_sys::console::log_1(&wasm_bindgen::JsValue::from_str("WORKER: Executor created"));

            let task = Task::without_notifications(
                "simple-task".to_string(),
                Configuration::default(),
                async move {
                    #[cfg(target_arch = "wasm32")]
                    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str("TASK: Inside async"));
                    done_tx.send(42);
                },
            );

            #[cfg(target_arch = "wasm32")]
            web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(
                "WORKER: About to spawn_static",
            ));

            //executor.spawn_static(task).detach();

            #[cfg(target_arch = "wasm32")]
            web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(
                "WORKER: spawn_static done",
            ));
        });

        log!("TEST: Thread spawned, awaiting result");
        let result = done_rx.await;
        log!("TEST: Got result: {}", result);
        assert_eq!(result, 42);
    }

    // === END ISOLATION TESTS ===

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_executor_notifier() {
        let mut executor = StaticLastResortExecutor::new();
        assert!(executor.executor_notifier().is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
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
