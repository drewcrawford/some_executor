// SPDX-License-Identifier: MIT OR Apache-2.0

//! WASM32-specific implementation for local task execution
//!
//! This module provides a WASM32-compatible implementation that avoids using
//! std::sync::Condvar and std::sync::Mutex, which fail on the main thread due
//! to the atomics.wait restriction in WebAssembly.
//!
//! Currently contains stubs that will be implemented later.

use crate::observer::ObserverNotified;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
pub use wasm_thread as thread;

/// WASM-specific newtype that wraps SpawnedStaticTask to implement Future
struct WasmStaticTaskFuture<F, N, E>
where
    F: Future + 'static,
    N: ObserverNotified<F::Output>,
    E: crate::SomeStaticExecutor,
    F::Output: 'static + Unpin,
{
    spawned: Pin<Box<crate::task::SpawnedStaticTask<F, N, E>>>,
}

impl<F, N, E> WasmStaticTaskFuture<F, N, E>
where
    F: Future + 'static,
    N: ObserverNotified<F::Output>,
    E: crate::SomeStaticExecutor,
    F::Output: 'static + Unpin,
{
    fn new(spawned: crate::task::SpawnedStaticTask<F, N, E>) -> Self {
        Self {
            spawned: Box::pin(spawned),
        }
    }
}

impl<F, N, E> Future for WasmStaticTaskFuture<F, N, E>
where
    F: Future + 'static,
    N: ObserverNotified<F::Output>,
    E: crate::SomeStaticExecutor,
    F::Output: 'static + Unpin,
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Delegate to the SpawnedStaticTask's poll method
        // Following the same pattern as in stdlib implementation
        self.spawned.as_mut().poll::<Infallible>(cx, None, None)
    }
}

/// Helper to run a SpawnedLocalTask to completion using WASM32-compatible mechanisms
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

/// Helper to run a SpawnedStaticTask to completion using WASM32-compatible mechanisms
pub(crate) fn run_static_task<F, N, E>(spawned: crate::task::SpawnedStaticTask<F, N, E>)
where
    F: Future + 'static,
    N: ObserverNotified<F::Output>,
    E: crate::SomeStaticExecutor,
    F::Output: 'static + Unpin,
{
    // Wrap the SpawnedStaticTask in our WASM-specific future wrapper
    let wasm_future = WasmStaticTaskFuture::new(spawned);

    // Use wasm-bindgen-futures::spawn_local to execute the future
    wasm_bindgen_futures::spawn_local(wasm_future);
}
