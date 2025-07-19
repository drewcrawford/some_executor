// SPDX-License-Identifier: MIT OR Apache-2.0

//! Comprehensive test for custom futures with yielding behavior across different executors.
//!
//! This test demonstrates:
//! 1. Custom Future that yields the first time it is polled
//! 2. Thread-based waking mechanism with delay
//! 3. Second poll returns Ready
//! 4. Testing across current_executor() and thread_static_executor() executors
//! 5. WASM-aware time and thread APIs
//! 6. Proper test completion mechanisms

use some_executor::{
    Instant, SomeExecutor, SomeStaticExecutor,
    current_executor::current_executor,
    observer::FinishedObservation,
    task::{Configuration, Task},
    thread_executor::thread_static_executor,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::task::{Context, Poll};
use test_executors::async_test;

//for the time being, wasm_thread only works in browser
//see https://github.com/rustwasm/wasm-bindgen/issues/4534,
//though we also need wasm_thread support.
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(not(target_arch = "wasm32"))]
use std::thread;
#[cfg(not(target_arch = "wasm32"))]
use std::time;
#[cfg(target_arch = "wasm32")]
use wasm_thread as thread;
#[cfg(target_arch = "wasm32")]
use web_time as time;

/// Custom Future that yields on first poll, then completes on second poll
/// with proper thread-based waking mechanism
struct YieldOnceFuture {
    poll_count: Arc<AtomicU32>,
    waker_spawned: Arc<AtomicBool>,
    result_value: i32,
}

impl YieldOnceFuture {
    fn new(result_value: i32) -> Self {
        Self {
            poll_count: Arc::new(AtomicU32::new(0)),
            waker_spawned: Arc::new(AtomicBool::new(false)),
            result_value,
        }
    }
}

impl Future for YieldOnceFuture {
    type Output = i32;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let count = self.poll_count.fetch_add(1, Ordering::Relaxed);

        match count {
            0 => {
                // First poll: yield and spawn waker thread
                if !self.waker_spawned.swap(true, Ordering::Relaxed) {
                    let waker = cx.waker().clone();

                    thread::spawn(move || {
                        // Small delay to simulate async work
                        thread::sleep(time::Duration::from_millis(10));
                        waker.wake();
                    });
                }
                Poll::Pending
            }
            _ => {
                // Second or later poll: complete
                Poll::Ready(self.result_value)
            }
        }
    }
}

/// Test custom future with current_executor()
#[async_test]
async fn test_yield_future_with_current_executor() {
    let start_time = Instant::now();

    // Create custom future that yields once
    let yield_future = YieldOnceFuture::new(42);
    let poll_counter = yield_future.poll_count.clone();

    // Create task with the future
    let task = Task::without_notifications(
        "yield-current-executor".to_string(),
        Configuration::default(),
        yield_future,
    );

    // Use current_executor() to spawn the task
    let mut executor = current_executor();
    let observer = executor.spawn(task);

    // Wait for completion and verify result
    let result = match observer.await {
        FinishedObservation::Ready(value) => value,
        FinishedObservation::Cancelled => panic!("Task was cancelled unexpectedly"),
    };
    let duration = start_time.elapsed();

    assert_eq!(result, 42);
    // Should have been polled at least twice (yield, then complete)
    assert!(poll_counter.load(Ordering::Relaxed) >= 2);

    // Should complete quickly but allow for thread scheduling
    assert!(duration.as_millis() < 1000);
}

/// Test custom future with thread_static_executor()
#[async_test]
async fn test_yield_future_with_thread_static_executor() {
    let start_time = Instant::now();

    // Create custom future that yields once
    let yield_future = YieldOnceFuture::new(99);
    let poll_counter = yield_future.poll_count.clone();

    // Create task with the future
    let task = Task::without_notifications(
        "yield-static-executor".to_string(),
        Configuration::default(),
        yield_future,
    );

    // Use thread_static_executor() to spawn the task (will use last resort if none set)
    let mut static_executor = thread_static_executor(|executor| executor.clone_box());
    let observer = static_executor.spawn_static(task);

    let result = match observer.await {
        FinishedObservation::Ready(value) => value,
        FinishedObservation::Cancelled => panic!("Task was cancelled unexpectedly"),
    };

    let duration = start_time.elapsed();

    assert_eq!(result, 99);
    // Should have been polled at least twice (yield, then complete)
    assert!(poll_counter.load(Ordering::Relaxed) >= 2);

    // Should complete quickly but allow for thread scheduling
    assert!(duration.as_millis() < 1000);
}

/// Test multiple yielding futures to verify robustness
#[async_test]
async fn test_multiple_yield_futures() {
    let start_time = Instant::now();

    // Create multiple yielding futures
    let futures_data: Vec<_> = (0..5)
        .map(|i| {
            let future = YieldOnceFuture::new(i * 10);
            let counter = future.poll_count.clone();
            (future, counter)
        })
        .collect();

    let mut results = Vec::new();
    let mut poll_counters = Vec::new();

    // Spawn all futures using current_executor
    let mut executor = current_executor();

    for (i, (future, counter)) in futures_data.into_iter().enumerate() {
        poll_counters.push(counter);

        let task = Task::without_notifications(
            format!("multi-yield-{}", i),
            Configuration::default(),
            future,
        );

        let observer = executor.spawn(task);
        let result = match observer.await {
            FinishedObservation::Ready(value) => value,
            FinishedObservation::Cancelled => panic!("Task {} was cancelled unexpectedly", i),
        };
        results.push(result);
    }

    let duration = start_time.elapsed();

    // Verify all results
    for (i, &result) in results.iter().enumerate() {
        assert_eq!(result, i as i32 * 10);
    }

    // Verify all futures were polled at least twice
    for (i, counter) in poll_counters.iter().enumerate() {
        assert!(
            counter.load(Ordering::Relaxed) >= 2,
            "Future {} was only polled {} times",
            i,
            counter.load(Ordering::Relaxed)
        );
    }

    // All should complete reasonably quickly
    assert!(duration.as_millis() < 2000);
}

/// Advanced future that demonstrates complex polling behavior
struct ComplexYieldFuture {
    poll_count: Arc<AtomicU32>,
    target_polls: u32,
    waker_spawned: Arc<AtomicBool>,
    value: String,
}

impl ComplexYieldFuture {
    fn new(target_polls: u32, value: String) -> Self {
        Self {
            poll_count: Arc::new(AtomicU32::new(0)),
            target_polls,
            waker_spawned: Arc::new(AtomicBool::new(false)),
            value,
        }
    }
}

impl Future for ComplexYieldFuture {
    type Output = String;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let count = self.poll_count.fetch_add(1, Ordering::Relaxed);

        if count >= self.target_polls {
            return Poll::Ready(format!(
                "{}-completed-after-{}-polls",
                self.value,
                count + 1
            ));
        }

        // Spawn waker thread for each poll until target is reached
        if !self.waker_spawned.swap(true, Ordering::Relaxed) {
            let waker = cx.waker().clone();
            let remaining = self.target_polls - count;

            thread::spawn(move || {
                // Staggered waking to test multiple poll cycles
                for i in 0..remaining {
                    thread::sleep(time::Duration::from_millis(5 + i as u64));
                    waker.wake_by_ref();
                }
            });
        }

        Poll::Pending
    }
}

/// Test complex polling behavior with both executor types
#[async_test]
async fn test_complex_yield_behavior() {
    // Test with current_executor
    let complex_future1 = ComplexYieldFuture::new(3, "current-exec".to_string());
    let task1 = Task::without_notifications(
        "complex-current".to_string(),
        Configuration::default(),
        complex_future1,
    );

    let mut executor = current_executor();
    let observer1 = executor.spawn(task1);
    let result1 = match observer1.await {
        FinishedObservation::Ready(value) => value,
        FinishedObservation::Cancelled => panic!("Complex task 1 was cancelled unexpectedly"),
    };

    assert!(result1.contains("current-exec-completed-after"));
    assert!(result1.contains("polls"));

    // Test with thread_static_executor
    let complex_future2 = ComplexYieldFuture::new(4, "static-exec".to_string());
    let task2 = Task::without_notifications(
        "complex-static".to_string(),
        Configuration::default(),
        complex_future2,
    );

    let mut static_executor2 = thread_static_executor(|executor| executor.clone_box());
    let observer2 = static_executor2.spawn_static(task2);

    let result2 = match observer2.await {
        FinishedObservation::Ready(value) => value,
        FinishedObservation::Cancelled => panic!("Complex task 2 was cancelled unexpectedly"),
    };

    assert!(result2.contains("static-exec-completed-after"));
    assert!(result2.contains("polls"));
}
