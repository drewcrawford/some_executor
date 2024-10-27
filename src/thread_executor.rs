//SPDX-License-Identifier: MIT OR Apache-2.0

use std::cell::RefCell;
use crate::{DynExecutor, DynLocalExecutor};

thread_local! {
    static THREAD_EXECUTOR: RefCell<Option<Box<DynExecutor>>> = RefCell::new(None);
    static THREAD_LOCAL_EXECUTOR: RefCell<Option<Box<DynLocalExecutor<'static>>>> = RefCell::new(None);
}

/**
Accesses the executor that is available for the current thread.
*/
pub fn thread_executor<R>(c: impl FnOnce(Option<&DynExecutor>) -> R) -> R {
    THREAD_EXECUTOR.with(|e| {
        c(e.borrow().as_ref().map(|e| &**e))
    })
}

/**
Sets the executor for the current thread.
*/
pub fn set_thread_executor(runtime: Box<DynExecutor>) {
    THREAD_EXECUTOR.with(|e| {
        *e.borrow_mut() = Some(runtime);
    });
}

/**
Accesses the local executor that is available for the current thread.
*/
pub fn thread_local_executor<R>(c: impl FnOnce(Option<&DynLocalExecutor>) -> R) -> R {
    THREAD_LOCAL_EXECUTOR.with(|e| {
        c(e.borrow().as_ref().map(|e| &**e))
    })
}

/**
Sets the local executor for the current thread.
*/
pub fn set_thread_local_executor(runtime: Box<DynLocalExecutor<'static>>) {
    THREAD_LOCAL_EXECUTOR.with(|e| {
        *e.borrow_mut() = Some(runtime);
    });
}

