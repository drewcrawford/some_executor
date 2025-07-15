// SPDX-License-Identifier: MIT OR Apache-2.0

//! WASM32-specific implementation for local task execution
//!
//! This module provides a WASM32-compatible implementation that avoids using
//! std::sync::Condvar and std::sync::Mutex, which fail on the main thread due
//! to the atomics.wait restriction in WebAssembly.
//!
//! Currently contains stubs that will be implemented later.

use crate::observer::ObserverNotified;
use std::future::Future;

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
