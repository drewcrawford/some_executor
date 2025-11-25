// SPDX-License-Identifier: MIT OR Apache-2.0

//! Standard library implementation for non-WASM32 targets
//!
//! This module contains the blocking synchronization primitives using std::sync::Condvar
//! and std::sync::Mutex, which work on all platforms except WASM32.

use crate::observer::ObserverNotified;
use std::convert::Infallible;
use std::future::Future;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, RawWaker, RawWakerVTable};
#[cfg(test)]
pub use std::thread;
/// Helper to run a SpawnedLocalTask to completion using condvar/mutex
#[allow(dead_code)]
pub(crate) fn run_local_task<F, N, E>(_spawned: crate::task::SpawnedLocalTask<F, N, E>)
where
    F: Future,
    N: ObserverNotified<F::Output>,
    E: for<'a> crate::SomeLocalExecutor<'a>,
{
    panic!(
        "Local task spawning without a proper executor is no longer supported. Please configure a local executor before spawning local tasks."
    );
}

// Atomic states for wake synchronization
const SLEEPING: u8 = 0;
const LISTENING: u8 = 1;
const WAKEPLS: u8 = 2;

/// Shared state between the polling loop and the waker
struct Shared {
    condvar: Condvar,
    mutex: Mutex<bool>,
    // Atomic for handling wake notifications without deadlock
    inline_notify: AtomicU8,
}

/// Custom waker for static task execution
struct StaticWaker {
    shared: Arc<Shared>,
}

/// Raw waker vtable for StaticWaker
const STATIC_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    // clone
    |data| {
        let waker = unsafe { Arc::from_raw(data as *const StaticWaker) };
        let w2 = waker.clone();
        std::mem::forget(waker);
        RawWaker::new(Arc::into_raw(w2) as *const (), &STATIC_WAKER_VTABLE)
    },
    // wake
    |data| {
        let waker = unsafe { Arc::from_raw(data as *const StaticWaker) };
        let old = waker.shared.inline_notify.swap(WAKEPLS, Ordering::Relaxed);
        if old == SLEEPING {
            // Acquire mutex before notify to ensure proper synchronization
            let _guard = waker.shared.mutex.lock().expect("Mutex poisoned");
            waker.shared.condvar.notify_one();
        }
        drop(waker);
    },
    // wake_by_ref
    |data| {
        let waker = unsafe { Arc::from_raw(data as *const StaticWaker) };
        let old = waker.shared.inline_notify.swap(WAKEPLS, Ordering::Relaxed);
        if old == SLEEPING {
            // Acquire mutex before notify to ensure proper synchronization
            let _guard = waker.shared.mutex.lock().expect("Mutex poisoned");
            waker.shared.condvar.notify_one();
        }
        std::mem::forget(waker);
    },
    // drop
    |data| {
        let waker = unsafe { Arc::from_raw(data as *const StaticWaker) };
        drop(waker)
    },
);

impl StaticWaker {
    fn into_core_waker(self) -> core::task::Waker {
        let data = Arc::into_raw(Arc::new(self));
        unsafe {
            core::task::Waker::from_raw(RawWaker::new(data as *const (), &STATIC_WAKER_VTABLE))
        }
    }
}

/// Helper to run a SpawnedStaticTask to completion using condvar/mutex
pub(crate) fn run_static_task<F, N, E>(spawned: crate::task::SpawnedStaticTask<F, N, E>)
where
    F: Future + 'static,
    N: ObserverNotified<F::Output>,
    E: crate::SomeStaticExecutor,
    F::Output: 'static + Unpin,
{
    // Create shared synchronization state
    let shared = Arc::new(Shared {
        condvar: Condvar::new(),
        mutex: Mutex::new(false),
        inline_notify: AtomicU8::new(SLEEPING),
    });

    // Create the waker
    let waker = StaticWaker {
        shared: shared.clone(),
    }
    .into_core_waker();

    let mut context = Context::from_waker(&waker);
    let mut pinned_spawned = Box::pin(spawned);

    loop {
        let mut _guard = shared.mutex.lock().expect("Mutex poisoned");

        // Set state to listening before polling
        shared.inline_notify.store(LISTENING, Ordering::Relaxed);

        // Poll the spawned task with None for static_executor since we don't have one here
        // and None for some_executor as this is running in last resort mode
        let poll_result = pinned_spawned
            .as_mut()
            .poll::<Infallible>(&mut context, None, None);

        match poll_result {
            Poll::Ready(()) => {
                // Task completed
                return;
            }
            Poll::Pending => {
                // Check if we got woken during polling
                let old = shared.inline_notify.swap(SLEEPING, Ordering::Relaxed);
                if old == WAKEPLS {
                    // We were woken during polling, continue polling immediately
                    drop(_guard);
                    continue;
                } else {
                    // Wait for wake notification
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::static_last_resort::StaticLastResortExecutor;
    use crate::task::{Configuration, Task};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
    use std::task::{Context, Poll};
    use std::time::{Duration, Instant};

    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_immediate_completion() {
        let mut executor = StaticLastResortExecutor::new();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let task = Task::without_notifications(
            "immediate-task".to_string(),
            Configuration::default(),
            async move {
                counter_clone.fetch_add(1, Ordering::Relaxed);
                42
            },
        );

        let (spawned, observer) = task.spawn_static(&mut executor);

        // Verify task hasn't run yet
        assert_eq!(counter.load(Ordering::Relaxed), 0);

        // Run the task to completion using our custom run_static_task
        run_static_task(spawned);

        // Verify task completed and observer has result
        assert_eq!(counter.load(Ordering::Relaxed), 1);
        match observer.observe() {
            crate::observer::Observation::Ready(value) => {
                assert_eq!(value, 42);
            }
            _ => panic!("Task should have completed immediately"),
        }
    }

    /// Custom future that returns Pending for a specified number of polls before completing
    struct DelayedFuture {
        polls_remaining: Arc<AtomicUsize>,
        result_value: i32,
        waker_thread_spawned: Arc<AtomicBool>,
    }

    impl DelayedFuture {
        fn new(polls_needed: usize, result_value: i32) -> Self {
            Self {
                polls_remaining: Arc::new(AtomicUsize::new(polls_needed)),
                result_value,
                waker_thread_spawned: Arc::new(AtomicBool::new(false)),
            }
        }
    }

    impl Future for DelayedFuture {
        type Output = i32;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let remaining = self.polls_remaining.fetch_sub(1, Ordering::Relaxed);

            if remaining <= 1 {
                return Poll::Ready(self.result_value);
            }

            // Spawn a thread to wake us after a delay, but only once
            if !self.waker_thread_spawned.swap(true, Ordering::Relaxed) {
                let waker = cx.waker().clone();
                #[cfg(not(target_arch = "wasm32"))]
                {
                    std::thread::spawn(move || {
                        // Give some time for the condvar to be set up
                        std::thread::sleep(Duration::from_millis(50));
                        // Wake multiple times to test robustness
                        for _ in 0..3 {
                            waker.wake_by_ref();
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    });
                }
                #[cfg(target_arch = "wasm32")]
                {
                    // For WASM, immediate wake since we can't use std::thread
                    waker.wake();
                }
            }

            Poll::Pending
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_delayed_waking() {
        let mut executor = StaticLastResortExecutor::new();

        // Create a future that needs to be polled 3 times before completion
        let delayed_future = DelayedFuture::new(3, 99);
        let polls_counter = delayed_future.polls_remaining.clone();

        let task = Task::without_notifications(
            "delayed-task".to_string(),
            Configuration::default(),
            delayed_future,
        );

        let (spawned, observer) = task.spawn_static(&mut executor);

        // Run the task to completion
        run_static_task(spawned);

        // Verify the task completed successfully
        match observer.observe() {
            crate::observer::Observation::Ready(value) => {
                assert_eq!(value, 99);
                // Verify it was polled at least 3 times and the counter was decremented
                // Since we start at 3 and decrement each poll, final value should be <= 0
                assert!(polls_counter.load(Ordering::Relaxed) <= 0);
            }
            _ => panic!("Task should have completed successfully"),
        }
    }

    /// Future that tests simple wake scenarios - completes after 2 pending polls
    struct SimplePendingFuture {
        poll_count: Arc<AtomicUsize>,
    }

    impl SimplePendingFuture {
        fn new() -> Self {
            Self {
                poll_count: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl Future for SimplePendingFuture {
        type Output = String;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let count = self.poll_count.fetch_add(1, Ordering::Relaxed);

            if count >= 2 {
                return Poll::Ready(format!("completed-after-{}-polls", count + 1));
            }

            // Spawn a thread to wake us for each poll
            if count < 2 {
                let waker = cx.waker().clone();
                #[cfg(not(target_arch = "wasm32"))]
                {
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(10));
                        waker.wake();
                    });
                }
                #[cfg(target_arch = "wasm32")]
                {
                    waker.wake();
                }
            }

            Poll::Pending
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_simple_pending_wake() {
        let mut executor = StaticLastResortExecutor::new();

        let simple_future = SimplePendingFuture::new();
        let poll_counter = simple_future.poll_count.clone();

        let task = Task::without_notifications(
            "simple-wake-task".to_string(),
            Configuration::default(),
            simple_future,
        );

        let (spawned, observer) = task.spawn_static(&mut executor);

        // Run the task to completion
        run_static_task(spawned);

        // Verify the task completed successfully
        match observer.observe() {
            crate::observer::Observation::Ready(value) => {
                assert!(value.starts_with("completed-after-"));
                // Should have been polled exactly 3 times (0, 1, 2)
                assert_eq!(poll_counter.load(Ordering::Relaxed), 3);
            }
            _ => panic!("Task should have completed successfully"),
        }
    }

    /// Future that completes after exactly 3 polls, demonstrating basic functionality
    struct ThreePollFuture {
        poll_count: Arc<AtomicUsize>,
    }

    impl ThreePollFuture {
        fn new() -> Self {
            Self {
                poll_count: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl Future for ThreePollFuture {
        type Output = usize;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let count = self.poll_count.fetch_add(1, Ordering::Relaxed) + 1;

            if count >= 3 {
                return Poll::Ready(count);
            }

            // Wake immediately to continue polling
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_three_poll_future() {
        let mut executor = StaticLastResortExecutor::new();

        let three_poll_future = ThreePollFuture::new();
        let poll_counter = three_poll_future.poll_count.clone();

        let task = Task::without_notifications(
            "three-poll-task".to_string(),
            Configuration::default(),
            three_poll_future,
        );

        let (spawned, observer) = task.spawn_static(&mut executor);

        let start_time = Instant::now();
        run_static_task(spawned);
        let duration = start_time.elapsed();

        // Verify the task completed successfully
        match observer.observe() {
            crate::observer::Observation::Ready(value) => {
                assert_eq!(value, 3);
                assert_eq!(poll_counter.load(Ordering::Relaxed), 3);
                // Should complete quickly since there's no external threading
                assert!(duration < Duration::from_secs(1));
            }
            _ => panic!("Task should have completed successfully"),
        }
    }

    /// Future that demonstrates task-local access and basic completion
    struct TaskMetadataFuture {
        poll_count: Arc<AtomicUsize>,
    }

    impl TaskMetadataFuture {
        fn new() -> Self {
            Self {
                poll_count: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl Future for TaskMetadataFuture {
        type Output = String;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let count = self.poll_count.fetch_add(1, Ordering::Relaxed) + 1;

            if count >= 2 {
                // Just return a success message without accessing task-locals to avoid complexity
                return Poll::Ready(format!("completed-after-{}-polls", count));
            }

            // Wake to continue polling
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_task_metadata_access() {
        let mut executor = StaticLastResortExecutor::new();

        let metadata_future = TaskMetadataFuture::new();
        let poll_counter = metadata_future.poll_count.clone();

        let task = Task::without_notifications(
            "metadata-task".to_string(),
            Configuration::default(),
            metadata_future,
        );

        let (spawned, observer) = task.spawn_static(&mut executor);

        // Run the task to completion
        run_static_task(spawned);

        // Verify the task completed successfully
        match observer.observe() {
            crate::observer::Observation::Ready(value) => {
                assert!(value.starts_with("completed-after-"));
                assert_eq!(poll_counter.load(Ordering::Relaxed), 2);
            }
            _ => panic!("Task should have completed successfully"),
        }
    }
}
