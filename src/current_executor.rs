//SPDX-License-Identifier: MIT OR Apache-2.0

use crate::{DynExecutor};

fn current_task_executor() -> Option<Box<DynExecutor>> {
    crate::task::TASK_EXECUTOR.with(|e| {
        match e {
            Some(e) => {
                //in task, but we may or may not have executor
                match e {
                    Some(e) => Some(e.clone_box()),
                    None => None
                }
            },
            None => None //not in a task
        }
    })
}

/**
Accesses the current executor.

# Implementation


*/
pub fn current_executor() -> Option<Box<DynExecutor>> {
    if let Some(executor) = current_task_executor() {
        Some(executor)
    } else if let Some(executor ) = crate::thread_executor::thread_executor(|e| e.map(|e| e.clone_box())) {
        Some(executor)
    } else {
        crate::global_executor::global_executor(|e| e.map(|e| e.clone_box()))
    }
}