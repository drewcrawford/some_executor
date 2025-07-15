// SPDX-License-Identifier: MIT OR Apache-2.0

//! Task-local variables and cancellation support for the some_executor framework.
//!
//! This module provides task-local variables that tasks can access during execution,
//! including task labels, priorities, cancellation status, and executor references.
//! It also provides the cancellation mechanism used by the task system.

use super::TaskID;
use crate::observer::ExecutorNotified;
use crate::{DynExecutor, Priority, SomeLocalExecutor, SomeStaticExecutor, task_local};
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

// Type aliases for complex types to satisfy clippy::type_complexity warnings

/// Type alias for a thread-local executor that can handle local tasks
type ThreadLocalExecutor = RefCell<
    Option<Box<dyn SomeLocalExecutor<'static, ExecutorNotifier = Box<dyn ExecutorNotified>>>>,
>;

/// Provides information about the cancellation status of a running task.
///
/// `InFlightTaskCancellation` allows tasks to check if they have been cancelled
/// and optionally exit early. This is particularly useful for long-running tasks
/// that should stop processing when no longer needed.
///
/// # Examples
///
/// Tasks can check their cancellation status via the task-local variable:
///
/// ```no_run
/// // Inside a task's future, IS_CANCELLED can be accessed:
///  use some_executor::task::IS_CANCELLED;
///  if IS_CANCELLED.with(|c| c.expect("No task").is_cancelled()) {
///      // Clean up and exit early
///      return;
///  }
/// ```
#[derive(Debug)]
pub struct InFlightTaskCancellation(Arc<AtomicBool>);

impl InFlightTaskCancellation {
    //we don't publish this so that we can change implementation later
    pub(crate) fn clone(&self) -> Self {
        InFlightTaskCancellation(self.0.clone())
    }

    pub(crate) fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Returns `true` if the task has been cancelled.
    ///
    /// Tasks can use this method to check if they should stop processing and return early.
    /// Checking for cancellation is optional - tasks that don't check will continue to
    /// run until completion.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // Inside a long-running task:
    /// # use some_executor::task::IS_CANCELLED;
    /// for i in 0..1000 {
    ///      if IS_CANCELLED.with(|c| c.expect("No task").is_cancelled() ) {
    ///          println!("Task cancelled at iteration {}", i);
    ///          break;
    ///      }
    ///      // Do some work...
    ///  }
    /// ```
    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

task_local! {
    /**
    Provides a debugging label to identify the current task.
    */
    pub static const TASK_LABEL: String;
    /**
    Provides a priority for the current task.
     */
    pub static const TASK_PRIORITY: Priority;
    /**
    Provides a mechanism for tasks to determine if they have been cancelled.

    Tasks can use this to determine if they should stop running.
    */
    pub static const IS_CANCELLED: InFlightTaskCancellation;

    /**
    Provides a unique identifier for the current task.
     */
    pub static const TASK_ID: TaskID;

    /**
    Provides an executor local to the current task.
    */
    pub static const TASK_EXECUTOR: Option<Box<DynExecutor>>;
}

task_local! {
    /**
    Provides a static executor local to the current task.

    This is similar to TASK_EXECUTOR but specifically for static executors
    that can handle !Send futures.
    */
    pub static const TASK_STATIC_EXECUTOR: Option<Box<dyn SomeStaticExecutor<ExecutorNotifier = Box<dyn ExecutorNotified>>>>;
}

thread_local! {
    pub static TASK_LOCAL_EXECUTOR: ThreadLocalExecutor = RefCell::new(None);
}

/*
InFlightTaskCancellation
- don't want to publish clone right now.  Eliminates Copy,Eq,Hash, etc.
Default is possible I suppose
 */
impl Default for InFlightTaskCancellation {
    fn default() -> Self {
        InFlightTaskCancellation(Arc::new(AtomicBool::new(false)))
    }
}

impl From<bool> for InFlightTaskCancellation {
    fn from(value: bool) -> Self {
        InFlightTaskCancellation(Arc::new(AtomicBool::new(value)))
    }
}

impl From<InFlightTaskCancellation> for bool {
    fn from(val: InFlightTaskCancellation) -> Self {
        val.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}
