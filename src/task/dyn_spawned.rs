// SPDX-License-Identifier: MIT OR Apache-2.0

//! Object-safe trait abstractions for spawned tasks.
//!
//! This module provides object-safe trait abstractions that allow executors to work
//! with spawned tasks without knowing their concrete future types. These traits enable
//! type erasure for heterogeneous collections of tasks, which is essential for building
//! custom executors that can manage multiple tasks with different future types.
//!
//! # Overview
//!
//! The traits in this module solve a fundamental problem in async Rust: how to store
//! and manage collections of tasks with different future types. Since Rust's type system
//! is static, executors need a way to work with tasks polymorphically. These traits
//! provide that capability through type erasure.
//!
//! # Main Components
//!
//! - [`DynSpawnedTask`]: Object-safe abstraction for [`SpawnedTask`] - used for Send futures
//! - [`DynLocalSpawnedTask`]: Object-safe abstraction for [`SpawnedLocalTask`] - used for !Send futures
//!
//! # Design Rationale
//!
//! These traits are designed to be:
//! - **Object-safe**: Can be used as trait objects (`dyn DynSpawnedTask`)
//! - **Minimal**: Only expose methods needed for task execution
//! - **Efficient**: Zero-cost abstraction when used with concrete types
//!
//! # Usage in Executors
//!
//! Custom executors typically store collections of `Box<dyn DynSpawnedTask>` or
//! `Box<dyn DynLocalSpawnedTask>` internally, allowing them to manage tasks of
//! different types uniformly.

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
/// # Purpose
///
/// `DynLocalSpawnedTask` enables executors to manage collections of local (non-Send) tasks
/// with different future types. This is particularly important for single-threaded executors
/// that need to work with futures containing `Rc`, `RefCell`, or other `!Send` types.
///
/// # Type Parameters
///
/// * `Executor` - The type of the local executor that will poll this task. This allows
///   the task to be aware of its executor type while maintaining type erasure for the
///   future itself.
///
/// # Usage in Custom Executors
///
/// This trait is primarily used internally by executors that need to store heterogeneous
/// collections of local tasks. The trait methods mirror those of [`SpawnedLocalTask`].
///
/// # Example
///
/// ```
/// use std::pin::Pin;
/// use some_executor::task::DynLocalSpawnedTask;
///
/// // Custom local executor storing type-erased tasks
/// struct CustomLocalExecutor<'future> {
///     tasks: Vec<Pin<Box<dyn DynLocalSpawnedTask<CustomLocalExecutor<'future>> + 'future>>>,
/// }
/// ```
///
/// # Implementation
///
/// This trait is automatically implemented for all [`SpawnedLocalTask`] instances where
/// the future, executor, and notifier types satisfy the appropriate bounds.
pub trait DynLocalSpawnedTask<Executor> {
    /// Polls the task with its associated executor.
    ///
    /// This method advances the task's future and manages task-local variables during
    /// execution. It should be called by the executor's main polling loop.
    ///
    /// # Arguments
    ///
    /// * `cx` - The waker context for async execution, containing the waker that will
    ///   be notified when the task is ready to make progress
    /// * `executor` - The local executor that owns this task, provided as a mutable
    ///   reference to allow the task to spawn additional work
    /// * `some_executor` - Optional executor for spawning new Send tasks from within
    ///   this task. Use `None` if spawning is not needed.
    ///
    /// # Returns
    ///
    /// - `Poll::Ready(())` when the task has completed
    /// - `Poll::Pending` when the task needs to be polled again later
    fn poll<'executor>(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        executor: &'executor mut Executor,
        some_executor: Option<Box<dyn SomeExecutor<ExecutorNotifier = Infallible> + 'static>>,
    ) -> std::task::Poll<()>;

    /// Returns the earliest time this task should be polled.
    ///
    /// This can be used by executors to implement delayed task execution or
    /// timer-based scheduling. Tasks with a future `poll_after` time should
    /// not be polled until that time has passed.
    fn poll_after(&self) -> crate::sys::Instant;

    /// Returns the task's label.
    ///
    /// The label is a human-readable identifier for the task, useful for
    /// debugging, logging, and monitoring task execution.
    fn label(&self) -> &str;

    /// Returns the task's unique identifier.
    ///
    /// Each task has a globally unique ID that can be used for tracking,
    /// correlation, and debugging purposes.
    fn task_id(&self) -> TaskID;

    /// Returns the task's execution hint.
    ///
    /// Hints provide guidance to the executor about how to schedule the task,
    /// such as whether it's CPU-intensive, I/O-bound, or has other special
    /// scheduling requirements.
    fn hint(&self) -> Hint;

    /// Returns the task's priority.
    ///
    /// Priority influences task scheduling order, with higher priority tasks
    /// typically being executed before lower priority ones, though the exact
    /// behavior depends on the executor implementation.
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
        some_executor: Option<Box<dyn SomeExecutor<ExecutorNotifier = Infallible> + 'static>>,
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
/// # Purpose
///
/// `DynSpawnedTask` enables multi-threaded executors to manage collections of tasks
/// with different future types while maintaining thread safety. The `Send` bound ensures
/// tasks can be moved between threads, which is essential for work-stealing schedulers
/// and thread pool executors.
///
/// # Type Parameters
///
/// * `LocalExecutorType` - The type of local executor that may be provided during polling.
///   This allows tasks to spawn additional local work if needed. Use [`Infallible`] if
///   no local executor is needed (which is the common case for most executors).
///
/// # Usage in Custom Executors
///
/// This trait is primarily used internally by executors that need to store heterogeneous
/// collections of tasks. The trait is `Send + Debug`, making it suitable for multi-threaded
/// executors.
///
/// # Examples
///
/// ## Basic Collection Storage
///
/// ```
/// use some_executor::task::DynSpawnedTask;
/// use std::convert::Infallible;
///
/// // Store a collection of type-erased tasks
/// let mut tasks: Vec<Box<dyn DynSpawnedTask<Infallible>>> = vec![];
/// ```
///
/// ## Custom Executor Implementation
///
/// ```
/// use some_executor::task::{DynSpawnedTask, SpawnedTask};
/// use std::convert::Infallible;
/// use std::pin::Pin;
/// use std::task::{Context, Poll};
/// use std::future::Future;
/// use some_executor::observer::ObserverNotified;
///
/// struct MyExecutor {
///     tasks: Vec<Pin<Box<dyn DynSpawnedTask<Infallible>>>>,
/// }
///
/// impl MyExecutor {
///     fn poll_all(&mut self, cx: &mut Context<'_>) {
///         for task in &mut self.tasks {
///             // Poll each task without a local executor
///             let _ = task.as_mut().poll(cx, None::<&mut Infallible>);
///         }
///     }
///
///     fn add_task<F, N, E>(&mut self, task: SpawnedTask<F, N, E>)
///     where
///         F: Future + Send + 'static,
///         F::Output: Send,
///         N: ObserverNotified<F::Output> + Send + 'static,
///         E: Send + 'static,
///     {
///         // Box the task and add it to our collection
///         self.tasks.push(Box::pin(task));
///     }
/// }
/// ```
///
/// # Implementation
///
/// This trait is automatically implemented for all [`SpawnedTask`] instances where the
/// future and notifier types are `Send`. This automatic implementation ensures that
/// any properly constructed `SpawnedTask` can be used polymorphically through this trait.
pub trait DynSpawnedTask<LocalExecutorType>: Send + Debug {
    /// Polls the task, optionally with a local executor context.
    ///
    /// This method advances the task's future and manages task-local variables during
    /// execution. It's the core method that executors call in their polling loops.
    ///
    /// # Arguments
    ///
    /// * `cx` - The waker context for async execution, containing the waker that will
    ///   be notified when the task is ready to make progress
    /// * `local_executor` - Optional local executor for spawning thread-local tasks.
    ///   This is rarely used; most executors pass `None`.
    ///
    /// # Returns
    ///
    /// - `Poll::Ready(())` when the task has completed (result sent to observers)
    /// - `Poll::Pending` when the task needs to be polled again later
    ///
    /// # Task-Local Context
    ///
    /// During polling, various task-local variables are set that the future can access,
    /// including task ID, label, priority, and cancellation status.
    fn poll<'l>(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        local_executor: Option<&mut LocalExecutorType>,
    ) -> std::task::Poll<()>
    where
        LocalExecutorType: SomeLocalExecutor<'l>;

    /// Returns the earliest time this task should be polled.
    ///
    /// This can be used by executors to implement delayed task execution or
    /// timer-based scheduling. Tasks with a future `poll_after` time should
    /// not be polled until that time has passed.
    fn poll_after(&self) -> crate::sys::Instant;

    /// Returns the task's label.
    ///
    /// The label is a human-readable identifier for the task, useful for
    /// debugging, logging, and monitoring task execution.
    fn label(&self) -> &str;

    /// Returns the task's unique identifier.
    ///
    /// Each task has a globally unique ID that can be used for tracking,
    /// correlation, and debugging purposes.
    fn task_id(&self) -> TaskID;

    /// Returns the task's execution hint.
    ///
    /// Hints provide guidance to the executor about how to schedule the task,
    /// such as whether it's CPU-intensive, I/O-bound, or has other special
    /// scheduling requirements.
    fn hint(&self) -> Hint;

    /// Returns the task's priority.
    ///
    /// Priority influences task scheduling order, with higher priority tasks
    /// typically being executed before lower priority ones, though the exact
    /// behavior depends on the executor implementation.
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
        self.poll(cx, local_executor)
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
