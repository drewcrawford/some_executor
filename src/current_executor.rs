//SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{DynExecutor};

/**
Accesses the current executor.

# Implementation


*/
pub fn current_executor() -> Option<Box<DynExecutor>> {
    if let Some(executor) = crate::task::TASK_EXECUTOR.with(|e| e.map(|e| e.clone_box())) {
        Some(executor)
    } else if let Some(executor ) = crate::thread_executor::thread_executor(|e| e.map(|e| e.clone_box())) {
        Some(executor)
    } else {
        crate::global_executor::global_executor(|e| e.map(|e| e.clone_box()))
    }
}