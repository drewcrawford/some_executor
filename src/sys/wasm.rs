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
    static RUNNING_TASKS: Cell<usize> = Cell::new(0);
    static INSTALLED_CLOSE_HANDLER: Cell<Option<Function>> = Cell::new(None);
    static CLOSE_IS_CALLED: Cell<bool> = Cell::new(false);
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
        let r = self.spawned.as_mut().poll::<Infallible>(cx, None, None);
        // web_sys::console::log_1(&"WasmStaticTask::poll done".into());
        r
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

/**
Patches the worker close function.

# Here be dragons!

Let me explain WTF is going on here.  When we execute futures with [wasm_bindgen_futures::spawn_local],
they need to be run by the event loop. Which means the event loop needs to um, actually run.

You may have some ideas about that like setTimeout, or requestAnimationFrame, but actually what happens
is when your thread is done, wasm_thread will call `close` on the worker, which will
cause the worker to exit immediately, without running any pending tasks and with no log or indication
of what happened.  Then you sit around wondering why your tasks are not running and trace them
through 20 layers of dependencies.

How do I know?  This is the kind of bug that I spent 3 days trying to figure out two years ago,
promptly forgot about, and then spent another 3 days trying to figure out again.

## The solution

We patch the `close` function on the global object to do two things:
1. Set a flag that close was called, so we can check it later.
2. Check if there are any running tasks. If there are, we do nothing and let the event loop run.
   If there are no running tasks, we call the original `close` function to actually close the worker.

This way, we ensure that the worker does not exit prematurely and all tasks are executed before the worker is closed.

Later when the last task is done, we call the original `close` function to actually close the worker.

## Alternatives considered

The usual hack is to use [wasm_bindgen::throw_str] to reject the close call, but that
is not a great idea as it shows up in console and prevents worker cleanup.
See https://github.com/rustwasm/wasm-bindgen/issues/2945 and
https://github.com/chemicstry/wasm_thread/issues/6.

I sent a PR to wasm_bindgen to document spawn_local's behavior to at least warn people,
but it was not merged.  See https://github.com/rustwasm/wasm-bindgen/pull/4391.

wasm_thread has some intention to "support async thread entrypoints"
which "would probably need a major rewrite", see https://github.com/chemicstry/wasm_thread/issues/10.

I am not optimistic about that because it's been several years with no real progress on that front.

There are two stale PRs in a related area that seem to mostly be stuck because any year now
we'll rewrite wasm_thread to support async thread entrypoints:

* https://github.com/chemicstry/wasm_thread/issues/10
* https://github.com/chemicstry/wasm_thread/pull/18

At this point I've lost enough time on this bug to actually consider implementing it myself,
but I'm not sure they have the review bandwidth to look at it, and this is simpler.
*/

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
