// SPDX-License-Identifier: MIT OR Apache-2.0

//! Object-safe trait abstractions for spawned tasks.
//!
//! This module provides object-safe trait abstractions that allow executors to work
//! with spawned tasks without knowing their concrete future types. These traits enable
//! type erasure for heterogeneous collections of tasks.
//!
//! The main traits provided are:
//! - [`DynSpawnedTask`]: Object-safe abstraction for [`SpawnedTask`]
//! - [`DynLocalSpawnedTask`]: Object-safe abstraction for [`SpawnedLocalTask`]

use super::{SpawnedLocalTask, SpawnedTask, TaskID};
use crate::hint::Hint;
use crate::observer::ObserverNotified;
use crate::{Priority, SomeExecutor, SomeLocalExecutor};
use std::convert::Infallible;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Object-safe type-erased wrapper for [`SpawnedLocalTask`].
///
/// This trait allows executors to work with spawned local tasks without knowing their
/// concrete future type. It provides all the essential operations needed to poll and
/// query task metadata in a type-erased manner.
///
/// # Usage
///
/// This trait is primarily used internally by executors that need to store heterogeneous
/// collections of local tasks. The trait methods mirror those of [`SpawnedLocalTask`].
///
/// # Implementation
///
/// This trait is automatically implemented for all [`SpawnedLocalTask`] instances.
pub trait DynLocalSpawnedTask<Executor> {
    /// Polls the task with its associated executor.
    ///
    /// # Arguments
    ///
    /// * `cx` - The waker context for async execution
    /// * `executor` - The local executor that owns this task
    /// * `some_executor` - Optional executor for spawning new tasks from within this task
    fn poll<'executor>(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        executor: &'executor mut Executor,
        some_executor: Option<Box<(dyn SomeExecutor<ExecutorNotifier = Infallible> + 'static)>>,
    ) -> std::task::Poll<()>;

    /// Returns the earliest time this task should be polled.
    fn poll_after(&self) -> crate::sys::Instant;

    /// Returns the task's label.
    fn label(&self) -> &str;

    /// Returns the task's unique identifier.
    fn task_id(&self) -> TaskID;

    /// Returns the task's execution hint.
    fn hint(&self) -> Hint;

    /// Returns the task's priority.
    fn priority(&self) -> Priority;
}

impl<'executor, F, ONotifier, Executor> DynLocalSpawnedTask<Executor>
    for SpawnedLocalTask<F, ONotifier, Executor>
where
    F: Future,
    Executor: SomeLocalExecutor<'executor>,
    ONotifier: ObserverNotified<F::Output>,
{
    fn poll<'ex>(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        executor: &'ex mut Executor,
        some_executor: Option<Box<(dyn SomeExecutor<ExecutorNotifier = Infallible> + 'static)>>,
    ) -> std::task::Poll<()> {
        SpawnedLocalTask::poll(self, cx, executor, some_executor)
    }

    fn poll_after(&self) -> crate::sys::Instant {
        self.poll_after
    }
    fn label(&self) -> &str {
        self.label()
    }

    fn hint(&self) -> Hint {
        self.hint
    }
    fn priority(&self) -> Priority {
        self.priority()
    }

    fn task_id(&self) -> TaskID {
        self.task_id()
    }
}

/// Object-safe type-erased wrapper for [`SpawnedTask`].
///
/// This trait allows executors to work with spawned tasks without knowing their
/// concrete future type. It provides all the essential operations needed to poll and
/// query task metadata in a type-erased manner.
///
/// # Type Parameters
///
/// * `LocalExecutorType` - The type of local executor that may be provided during polling.
///   Use [`Infallible`] if no local executor is needed.
///
/// # Usage
///
/// This trait is primarily used internally by executors that need to store heterogeneous
/// collections of tasks. The trait is `Send + Debug`, making it suitable for multi-threaded
/// executors.
///
/// # Examples
///
/// ```
/// use some_executor::task::DynSpawnedTask;
/// use std::convert::Infallible;
///
/// // Store a collection of type-erased tasks
/// let tasks: Vec<Box<dyn DynSpawnedTask<Infallible>>> = vec![];
/// ```
///
/// # Implementation
///
/// This trait is automatically implemented for all [`SpawnedTask`] instances where the
/// future and notifier types are `Send`.
pub trait DynSpawnedTask<LocalExecutorType>: Send + Debug {
    /// Polls the task, optionally with a local executor context.
    ///
    /// # Arguments
    ///
    /// * `cx` - The waker context for async execution
    /// * `local_executor` - Optional local executor for thread-local operations
    fn poll<'l>(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        local_executor: Option<&mut LocalExecutorType>,
    ) -> std::task::Poll<()>
    where
        LocalExecutorType: SomeLocalExecutor<'l>;

    /// Returns the earliest time this task should be polled.
    fn poll_after(&self) -> crate::sys::Instant;

    /// Returns the task's label.
    fn label(&self) -> &str;

    /// Returns the task's unique identifier.
    fn task_id(&self) -> TaskID;

    /// Returns the task's execution hint.
    fn hint(&self) -> Hint;

    /// Returns the task's priority.
    fn priority(&self) -> Priority;
}

impl<F: Future, N: ObserverNotified<<F as Future>::Output>, E, L> DynSpawnedTask<L>
    for SpawnedTask<F, N, E>
where
    F: Send,
    E: Send,
    N: Send,
    F::Output: Send,
{
    fn poll<'l>(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        local_executor: Option<&mut L>,
    ) -> Poll<()>
    where
        L: SomeLocalExecutor<'l>,
    {
        SpawnedTask::poll(self, cx, local_executor)
    }

    fn poll_after(&self) -> crate::sys::Instant {
        self.poll_after()
    }

    fn label(&self) -> &str {
        self.label()
    }

    fn task_id(&self) -> TaskID {
        self.task_id()
    }

    fn hint(&self) -> Hint {
        self.hint()
    }

    fn priority(&self) -> Priority {
        self.priority()
    }
}
