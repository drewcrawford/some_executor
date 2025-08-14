// SPDX-License-Identifier: MIT OR Apache-2.0

//! WASM32-specific implementation for local task execution
//!
//! This module provides a WASM32-compatible implementation that avoids using
//! std::sync::Condvar and std::sync::Mutex, which fail on the main thread due
//! to the atomics.wait restriction in WebAssembly.
//!
//! Currently contains stubs that will be implemented later.

use crate::observer::ObserverNotified;
use js_sys::Function;
use js_sys::Reflect;
use js_sys::wasm_bindgen;
use std::cell::Cell;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
#[cfg(test)]
pub use wasm_thread as thread;

thread_local! {
    static RUNNING_TASKS: Cell<usize> = const { Cell::new(0) };
    static INSTALLED_CLOSE_HANDLER: Cell<Option<Function>> = const { Cell::new(None) };
    static CLOSE_IS_CALLED: Cell<bool> = const { Cell::new(false) };
}

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
        // web_sys::console::log_1(&"WasmStaticTask::poll".into());

        // Delegate to the SpawnedStaticTask's poll method
        // Following the same pattern as in stdlib implementation

        // web_sys::console::log_1(&"WasmStaticTask::poll done".into());
        self.spawned.as_mut().poll::<Infallible>(cx, None, None)
    }
}

impl<F, N, E> Drop for WasmStaticTaskFuture<F, N, E>
where
    F: Future + 'static,
    N: ObserverNotified<F::Output>,
    E: crate::SomeStaticExecutor,
    F::Output: 'static + Unpin,
{
    fn drop(&mut self) {
        RUNNING_TASKS.with(|running| {
            running.update(|count| count - 1);
        });
        consider_closing();
    }
}

/// Helper to run a SpawnedLocalTask to completion using WASM32-compatible mechanisms
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

fn patch_if_needed() {
    INSTALLED_CLOSE_HANDLER.with(|installed| {
        let t = installed.take();
        if t.is_some() {
            //set back
            INSTALLED_CLOSE_HANDLER.set(t);
        } else {
            let global = js_sys::global();

            // 2. Grab the original `close` function (`fn () -> !`)
            let orig_close: Function = match Reflect::get(&global, &"close".into())
                .expect("global.close should exist")
                .dyn_into()
            {
                Ok(f) => f,
                Err(_) => {
                    web_sys::console::error_1(
                        &"Global close function not found; might be node?".into(),
                    );
                    return;
                }
            };

            // 3. Make a Rust closure that logs *then* calls the real close
            let wrapper = Closure::wrap(Box::new(move || {
                CLOSE_IS_CALLED.with(|c| c.set(true));
                consider_closing();
            }) as Box<dyn Fn()>);

            // 4. Replace `self.close` with our wrapper
            Reflect::set(&global, &"close".into(), wrapper.as_ref().unchecked_ref())
                .expect("failed to patch close");

            // 5. Prevent the wrapper from being dropped
            wrapper.forget();
            installed.set(Some(orig_close));
        }
    });
}

fn consider_closing() {
    if !CLOSE_IS_CALLED.with(|c| c.get()) {
        // If close was not called, we do nothing
        return;
    }
    RUNNING_TASKS.with(|running| {
        let count = running.get();
        if count > 0 {
            // If there are still running tasks, we do nothing
            return;
        }
        // If no tasks are running, we can safely call the original close
        INSTALLED_CLOSE_HANDLER.with(|installed| {
            if let Some(orig_close) = installed.take() {
                orig_close
                    .call0(&js_sys::global())
                    .expect("Failed to call original close");
            }
        });
    });
}

/// Helper to run a SpawnedStaticTask to completion using WASM32-compatible mechanisms
pub(crate) fn run_static_task<F, N, E>(spawned: crate::task::SpawnedStaticTask<F, N, E>)
where
    F: Future + 'static,
    N: ObserverNotified<F::Output>,
    E: crate::SomeStaticExecutor,
    F::Output: 'static + Unpin,
{
    patch_if_needed();

    // Wrap the SpawnedStaticTask in our WASM-specific future wrapper
    let wasm_future = WasmStaticTaskFuture::new(spawned);

    RUNNING_TASKS.with(|running| {
        running.update(|count| count + 1);
    });
    // Use wasm-bindgen-futures::spawn_local to execute the future
    wasm_bindgen_futures::spawn_local(wasm_future);
}
