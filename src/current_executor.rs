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

1.  If the current task has an executor, it is returned.
2.  If the current thread has an executor, it is returned.
3.  If the global executor is set, it is returned.
4.  Otherwise, a last resort executor is returned.

*/
pub fn current_executor() -> Box<DynExecutor> {
    if let Some(executor) = current_task_executor() {
        executor
    } else if let Some(executor ) = crate::thread_executor::thread_executor(|e| e.map(|e| e.clone_box())) {
        executor
    } else if let Some(executor) = crate::global_executor::global_executor(|e| e.map(|e| e.clone_box())) {
        executor
    }
    else {
        Box::new(crate::last_resort::LastResortExecutor::new())
    }
}