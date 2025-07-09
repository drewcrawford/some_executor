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
use std::pin::Pin;
use std::task::{Context, Poll};

type TaskType =
    crate::task::SpawnedLocalTask<F, N, crate::local_last_resort::LocalLastResortExecutor>;

/// Helper to run a SpawnedLocalTask to completion using WASM32-compatible mechanisms
///
/// This function will be implemented to avoid using std::sync::Condvar and std::sync::Mutex,
/// which fail on the WASM32 main thread due to atomics.wait restrictions.
///
/// TODO: Implement using one of these approaches:
/// - wasm_bindgen_futures::spawn_local with state coordination
/// - Polling loop with setTimeout-based yields
/// - Message passing with async coordination
pub(crate) fn run_local_task<F, N>(spawned: TaskType)
where
    F: Future,
    N: ObserverNotified<F::Output>,
{
    todo!()
}
