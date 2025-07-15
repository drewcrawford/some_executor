// SPDX-License-Identifier: MIT OR Apache-2.0

//! Standard library implementation for non-WASM32 targets
//!
//! This module contains the blocking synchronization primitives using std::sync::Condvar
//! and std::sync::Mutex, which work on all platforms except WASM32.

use crate::observer::ObserverNotified;
use std::future::Future;

/// Helper to run a SpawnedLocalTask to completion using condvar/mutex
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

/// Helper to run a SpawnedStaticTask to completion using condvar/mutex
pub(crate) fn run_static_task<F, N, E>(_spawned: crate::task::SpawnedStaticTask<F, N, E>)
where
    F: Future + 'static,
    N: ObserverNotified<F::Output>,
    E: crate::SomeStaticExecutor,
    F::Output: 'static + Unpin,
{
    panic!(
        "Static task spawning without a proper executor is no longer supported. Please configure a static executor before spawning static tasks."
    );
}
