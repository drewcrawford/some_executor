//! Task spawning functionality.
//!
//! This module contains all the spawn-related methods and type aliases for tasks.
//! It provides methods for spawning tasks onto different types of executors:
//! regular executors, local executors, and static executors, along with their
//! object-safe variants.

use crate::dyn_observer_notified::{ObserverNotifiedErased, ObserverNotifiedErasedLocal};
use crate::observer::{ExecutorNotified, ObserverNotified, TypedObserver, observer_channel};
use crate::task::task_local::InFlightTaskCancellation;
use crate::task::{Task, spawned};
use crate::{ObjSafeStaticTask, SomeExecutor, SomeLocalExecutor, SomeStaticExecutor};
use std::any::Any;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;

// Type aliases for complex spawn result types

/// Type alias for the result of spawning a task onto an executor
/// Returns a tuple of (SpawnedTask, TypedObserver)
pub type SpawnResult<F, N, Executor> = (
    spawned::SpawnedTask<F, N, Executor>,
    TypedObserver<<F as Future>::Output, <Executor as SomeExecutor>::ExecutorNotifier>,
);

/// Type alias for the result of spawning a task onto a local executor
/// Returns a tuple of (SpawnedLocalTask, TypedObserver)
pub type SpawnLocalResult<'a, F, N, Executor> = (
    spawned::SpawnedLocalTask<F, N, Executor>,
    TypedObserver<<F as Future>::Output, <Executor as SomeLocalExecutor<'a>>::ExecutorNotifier>,
);

/// Type alias for the result of spawning a task on a static executor
/// Returns a tuple with a spawned static task and observer
pub type SpawnStaticResult<F, N, Executor> = (
    spawned::SpawnedStaticTask<F, N, Executor>,
    TypedObserver<<F as Future>::Output, <Executor as SomeStaticExecutor>::ExecutorNotifier>,
);

/// Type alias for the result of spawning a task using object-safe method
/// Returns a tuple with a boxed executor notifier
pub type SpawnObjSafeResult<F, N, Executor> = (
    spawned::SpawnedTask<F, N, Executor>,
    TypedObserver<<F as Future>::Output, Box<dyn ExecutorNotified + Send>>,
);

/// Type alias for the result of spawning a local task using object-safe method
/// Returns a tuple with a boxed executor notifier
pub type SpawnLocalObjSafeResult<F, N, Executor> = (
    spawned::SpawnedLocalTask<F, N, Executor>,
    TypedObserver<<F as Future>::Output, Box<dyn ExecutorNotified>>,
);

/// Type alias for the result of spawning a static task using object-safe method
/// Returns a tuple with a boxed executor notifier
pub type SpawnStaticObjSafeResult<F, N, Executor> = (
    spawned::SpawnedStaticTask<F, N, Executor>,
    TypedObserver<<F as Future>::Output, Box<dyn ExecutorNotified>>,
);

/// Type alias for a boxed future that outputs boxed Any and is Send + 'static
type BoxedSendFuture =
    Pin<Box<dyn Future<Output = Box<dyn Any + 'static + Send>> + 'static + Send>>;

/// Type alias for a boxed observer notifier that handles Send Any values
type BoxedSendObserverNotifier = Box<dyn ObserverNotified<dyn Any + Send> + Send>;

/// Type alias for a Task that can be used with object-safe spawning
type ObjSafeTask = Task<BoxedSendFuture, BoxedSendObserverNotifier>;

/// Type alias for a boxed future that outputs boxed Any (non-Send)
type BoxedLocalFuture = Pin<Box<dyn Future<Output = Box<dyn Any + 'static>> + 'static>>;

/// Type alias for a boxed observer notifier that handles Any values (non-Send)
type BoxedLocalObserverNotifier = Box<dyn ObserverNotified<dyn Any + 'static>>;

/// Type alias for a Task that can be used with local object-safe spawning
type ObjSafeLocalTask = Task<BoxedLocalFuture, BoxedLocalObserverNotifier>;

impl<F: Future, N> Task<F, N> {
    /// Spawns the task onto an executor.
    ///
    /// This method transfers ownership of the task to the executor, which will poll it
    /// to completion. It returns a tuple containing:
    ///
    /// 1. A [`SpawnedTask`] that can be polled by the executor
    /// 2. A [`TypedObserver`] that can be used to await or check the task's completion
    ///
    /// # Arguments
    ///
    /// * `executor` - The executor that will run this task
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::any::Any;
    /// # use std::convert::Infallible;
    /// use std::future::Future;
    /// use std::pin::Pin;
    /// use some_executor::observer::{FinishedObservation, Observer, ObserverNotified, TypedObserver};
    /// use some_executor::task::{Task, Configuration};
    /// use some_executor::SomeExecutor;
    ///
    /// # #[derive(Debug)]
    /// # struct MyExecutor;
    /// # impl SomeExecutor for MyExecutor {
    /// #     type ExecutorNotifier = std::convert::Infallible;
    /// #     fn spawn<F, N>(&mut self, task: Task<F, N>) -> impl some_executor::observer::Observer<Value = F::Output>
    /// #     where F: std::future::Future + Send + 'static, N: some_executor::observer::ObserverNotified<F::Output> + Send + 'static, F::Output: Send + 'static
    /// #     { todo!() as TypedObserver::<F::Output, Infallible> }
    /// #     fn spawn_async<F, N>(&mut self, task: Task<F, N>) -> impl std::future::Future<Output = impl some_executor::observer::Observer<Value = F::Output>>
    /// #     where F: std::future::Future + Send + 'static, N: some_executor::observer::ObserverNotified<F::Output> + Send + 'static, F::Output: Send + 'static  
    /// #     { async { todo!() as TypedObserver::<F::Output, Infallible>} }
    /// #     fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> { None }
    /// #     fn clone_box(&self) -> Box<dyn SomeExecutor<ExecutorNotifier = Self::ExecutorNotifier>> { todo!() }
    ///
    /// #     fn spawn_objsafe_async<'s>(&'s mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any + 'static + Send>> + 'static + Send>>, Box<dyn ObserverNotified<dyn Any + Send> + Send>>) -> Box<dyn Future<Output=Box<dyn Observer<Value=Box<dyn Any + Send>, Output=FinishedObservation<Box<dyn Any + Send>>> + Send>> + 's> {
    /// #        todo!()
    /// #     }
    ///
    /// #     fn spawn_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any + 'static + Send>> + 'static + Send>>, Box<dyn ObserverNotified<dyn Any + Send> + Send>>) -> Box<dyn Observer<Value=Box<dyn Any + Send>, Output=FinishedObservation<Box<dyn Any + Send>>> + Send> {
    /// #         todo!()
    /// #     }
    /// # }
    /// # async fn example() {
    /// let task = Task::without_notifications(
    ///     "compute".to_string(),
    ///     Configuration::default(),
    ///     async { 2 + 2 },
    /// );
    ///
    /// let mut executor = MyExecutor;
    /// let (spawned, observer) = task.spawn(&mut executor);
    ///
    /// // The task is now owned by the executor
    /// // Use the observer to check completion
    /// # }
    /// ```
    ///
    /// # Note
    ///
    /// When using this method, the `TASK_LOCAL_EXECUTOR` will be set to `None`.
    /// To spawn a task onto a local executor instead, use [`spawn_local`](Self::spawn_local).
    pub fn spawn<Executor: SomeExecutor>(
        mut self,
        executor: &mut Executor,
    ) -> SpawnResult<F, N, Executor> {
        let cancellation = InFlightTaskCancellation::default();
        let some_notifier: Option<Executor::ExecutorNotifier> = executor.executor_notifier();
        let task_id = self.task_id();
        let (sender, receiver) = observer_channel(
            self.notifier.take(),
            some_notifier,
            cancellation.clone(),
            task_id,
        );
        let boxed_executor = executor.clone_box();
        let spawned_task = spawned::SpawnedTask {
            task: self.future,
            sender,
            phantom: PhantomData,
            poll_after: self.poll_after,
            hint: self.hint,
            label: Some(self.label),
            priority: self.priority,
            task_id,
            cancellation: Some(cancellation),
            executor: Some(boxed_executor),
        };
        (spawned_task, receiver)
    }

    /// Spawns the task onto a local executor.
    ///
    /// Local executors are tied to a specific thread and cannot be sent across threads.
    /// This method is similar to [`spawn`](Self::spawn) but works with executors that
    /// are not `Send`.
    ///
    /// # Arguments
    ///
    /// * `executor` - The local executor that will run this task
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// 1. A [`SpawnedLocalTask`] that can be polled by the executor
    /// 2. A [`TypedObserver`] that can be used to await or check the task's completion
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration};
    /// # use some_executor::SomeLocalExecutor;
    /// # use some_executor::observer::{Observer, ObserverNotified, FinishedObservation, TypedObserver};
    /// # use std::any::Any;
    /// # use std::pin::Pin;
    /// # use std::future::Future;
    /// # use std::convert::Infallible;
    ///
    /// # #[derive(Debug)]
    /// # struct MyLocalExecutor;
    /// # impl<'a> SomeLocalExecutor<'a> for MyLocalExecutor {
    /// #     type ExecutorNotifier = Infallible;
    /// #     fn spawn_local<F: Future, N: ObserverNotified<F::Output>>(&mut self, task: some_executor::task::Task<F, N>) -> impl Observer<Value = F::Output> where F: 'a, F::Output: 'static {
    /// #         todo!() as TypedObserver::<F::Output, Infallible>
    /// #     }
    /// #     fn spawn_local_async<F: Future, N: ObserverNotified<F::Output>>(&mut self, task: some_executor::task::Task<F, N>) -> impl Future<Output = impl Observer<Value = F::Output>> where F: 'a, F::Output: 'static {
    /// #         async { todo!() as TypedObserver::<F::Output, Infallible> }
    /// #     }
    /// #     fn spawn_local_objsafe(&mut self, task: some_executor::task::Task<Pin<Box<dyn Future<Output = Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Observer<Value = Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>> { todo!() }
    /// #     fn spawn_local_objsafe_async<'s>(&'s mut self, task: some_executor::task::Task<Pin<Box<dyn Future<Output = Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Future<Output = Box<dyn Observer<Value = Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>>> + 's> { Box::new(async { todo!() }) }
    /// #     fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> { None }
    /// # }
    /// let mut executor = MyLocalExecutor;
    ///
    /// let task = Task::without_notifications(
    ///     "local-work".to_string(),
    ///     Configuration::default(),
    ///     async {
    ///         // Can access thread-local data here
    ///         println!("Running on the local thread");
    ///     },
    /// );
    ///
    /// let (spawned, observer) = task.spawn_local(&mut executor);
    /// ```
    pub fn spawn_local<'executor, Executor: SomeLocalExecutor<'executor>>(
        mut self,
        executor: &mut Executor,
    ) -> SpawnLocalResult<'executor, F, N, Executor> {
        let cancellation = InFlightTaskCancellation::default();
        let task_id = self.task_id();
        let (sender, receiver) = observer_channel(
            self.notifier.take(),
            executor.executor_notifier(),
            cancellation.clone(),
            task_id,
        );
        let spawned_task = spawned::SpawnedLocalTask {
            task: self.future,
            sender,
            executor: PhantomData,
            poll_after: self.poll_after,
            hint: self.hint,
            priority: self.priority,
            label: Some(self.label),
            task_id,
            cancellation: Some(cancellation),
        };
        (spawned_task, receiver)
    }

    /// Spawns the task onto a static executor.
    ///
    /// Static executors handle futures that are `'static` but not necessarily `Send`.
    /// This is useful for thread-local executors that work with static data but don't
    /// need to cross thread boundaries.
    ///
    /// # Arguments
    ///
    /// * `executor` - The static executor that will run this task
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// 1. A [`SpawnedStaticTask`] that can be polled by the executor
    /// 2. A [`TypedObserver`] that can be used to await or check the task's completion
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration};
    /// # use some_executor::SomeStaticExecutor;
    /// # use some_executor::observer::{Observer, ObserverNotified, FinishedObservation, TypedObserver};
    /// # use std::any::Any;
    /// # use std::pin::Pin;
    /// # use std::future::Future;
    /// # use std::convert::Infallible;
    ///
    /// # #[derive(Debug)]
    /// # struct MyStaticExecutor;
    /// # impl SomeStaticExecutor for MyStaticExecutor {
    /// #     type ExecutorNotifier = Box<dyn some_executor::observer::ExecutorNotified>;
    /// #     fn spawn_static<F: Future, N: ObserverNotified<F::Output>>(&mut self, task: some_executor::task::Task<F, N>) -> impl Observer<Value = F::Output> where F: 'static, F::Output: 'static {
    /// #         todo!() as TypedObserver::<F::Output, Box<dyn some_executor::observer::ExecutorNotified>>
    /// #     }
    /// #     fn spawn_static_async<F: Future, N: ObserverNotified<F::Output>>(&mut self, task: some_executor::task::Task<F, N>) -> impl Future<Output = impl Observer<Value = F::Output>> where F: 'static, F::Output: 'static {
    /// #         async { todo!() as TypedObserver::<F::Output, Box<dyn some_executor::observer::ExecutorNotified>> }
    /// #     }
    /// #     fn spawn_static_objsafe(&mut self, task: some_executor::task::Task<Pin<Box<dyn Future<Output = Box<dyn Any + 'static>> + 'static>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Observer<Value = Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>> { todo!() }
    /// #     fn spawn_static_objsafe_async<'s>(&'s mut self, task: some_executor::task::Task<Pin<Box<dyn Future<Output = Box<dyn Any + 'static>> + 'static>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Future<Output = Box<dyn Observer<Value = Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>>> + 's> { Box::new(async { todo!() }) }
    /// #     fn clone_box(&self) -> Box<some_executor::DynStaticExecutor> { todo!() }
    /// #     fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> { None }
    /// # }
    /// let mut executor = MyStaticExecutor;
    ///
    /// let task = Task::without_notifications(
    ///     "static-work".to_string(),
    ///     Configuration::default(),
    ///     async {
    ///         // Can access static data here
    ///         println!("Running on static executor");
    ///     },
    /// );
    ///
    /// let (spawned, observer) = task.spawn_static(&mut executor);
    /// ```
    pub fn spawn_static<Executor: SomeStaticExecutor>(
        mut self,
        executor: &mut Executor,
    ) -> SpawnStaticResult<F, N, Executor> {
        let cancellation = InFlightTaskCancellation::default();
        let task_id = self.task_id();
        let (sender, receiver) = observer_channel(
            self.notifier.take(),
            executor.executor_notifier(),
            cancellation.clone(),
            task_id,
        );
        let spawned_task = spawned::SpawnedStaticTask {
            task: self.future,
            sender,
            executor: PhantomData,
            poll_after: self.poll_after,
            hint: self.hint,
            priority: self.priority,
            label: Some(self.label),
            task_id,
            cancellation: Some(cancellation),
        };
        (spawned_task, receiver)
    }

    /**
    Spawns the task onto a local executor.

    # Objsafe

    A word on exactly what 'objsafe' means in this context.  Objsafe means that whoever is spawning the task,
    doesn't know which executor they are using, so they spawn onto an objsafe executor via the objsafe methods.

    This has two implications.  First, we need to hide the executor type from the spawner.  However, we don't need
    to hide it from the *executor*, since the executor knows what it is.  Accordingly, this information is erased
    with respect to types sent to the spawner, and not erased with respect to types sent
    to the executor.

    Second, the objsafe spawn method cannot have any generics.  Therefore, the future type is erased (boxed) and worse,
    the output type is erased as well.  Accordingly we do not know what it is.
    */
    pub fn spawn_objsafe<Executor: SomeExecutor>(
        mut self,
        executor: &mut Executor,
    ) -> SpawnObjSafeResult<F, N, Executor> {
        let cancellation = InFlightTaskCancellation::default();
        let boxed_executor_notifier = executor
            .executor_notifier()
            .map(|n| Box::new(n) as Box<dyn ExecutorNotified + Send>);
        let boxed_executor = executor.clone_box();
        let (sender, receiver) = observer_channel(
            self.notifier.take(),
            boxed_executor_notifier,
            cancellation.clone(),
            self.task_id,
        );
        let spawned_task = spawned::SpawnedTask {
            task: self.future,
            sender,
            phantom: PhantomData,
            poll_after: self.poll_after,
            hint: self.hint,
            label: Some(self.label),
            priority: self.priority,
            task_id: self.task_id,
            cancellation: Some(cancellation),
            executor: Some(boxed_executor),
        };
        (spawned_task, receiver)
    }

    /**
    Spawns the task onto a local executor.

    # Objsafe

    A word on exactly what 'objsafe' means in this context.  Objsafe means that whoever is spawning the task,
    doesn't know which executor they are using, so they spawn onto an objsafe executor via the objsafe methods.

    This has two implications.  First, we need to hide the executor type from the spawner.  However, we don't need
    to hide it from the *executor*, since the executor knows what it is.  Accordingly this information is erased
    with respect to types sent to the spawner, and not erased with respect to types sent
    to the executor.

    Second, the objsafe spawn method cannot have any generics.  Therefore, the future type is erased (boxed) and worse,
    the output type is erased as well.  Accordingly we do not know what it is.
    */
    pub fn spawn_local_objsafe<'executor, Executor: SomeLocalExecutor<'executor>>(
        mut self,
        executor: &mut Executor,
    ) -> SpawnLocalObjSafeResult<F, N, Executor> {
        let cancellation = InFlightTaskCancellation::default();
        let task_id = self.task_id();

        let boxed_executor_notifier = executor
            .executor_notifier()
            .map(|n| Box::new(n) as Box<dyn ExecutorNotified>);
        let (sender, receiver) = observer_channel(
            self.notifier.take(),
            boxed_executor_notifier,
            cancellation.clone(),
            task_id,
        );
        let spawned_task = spawned::SpawnedLocalTask {
            task: self.future,
            sender,
            poll_after: self.poll_after,
            hint: self.hint,
            priority: self.priority,
            executor: PhantomData,
            label: Some(self.label),
            task_id,
            cancellation: Some(cancellation),
        };
        (spawned_task, receiver)
    }

    /// Spawns the task onto a static executor using object-safe method.
    ///
    /// This method is similar to [`spawn_static`](Self::spawn_static) but uses type erasure
    /// to support object-safe trait usage.
    ///
    /// # Arguments
    ///
    /// * `executor` - The static executor that will run this task
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// 1. A [`SpawnedStaticTask`] that can be polled by the executor
    /// 2. A [`TypedObserver`] that can be used to await or check the task's completion
    pub fn spawn_static_objsafe<Executor: SomeStaticExecutor>(
        mut self,
        executor: &mut Executor,
    ) -> SpawnStaticObjSafeResult<F, N, Executor> {
        let cancellation = InFlightTaskCancellation::default();
        let task_id = self.task_id();

        let boxed_executor_notifier = executor
            .executor_notifier()
            .map(|n| Box::new(n) as Box<dyn ExecutorNotified>);
        let (sender, receiver) = observer_channel(
            self.notifier.take(),
            boxed_executor_notifier,
            cancellation.clone(),
            task_id,
        );
        let spawned_task = spawned::SpawnedStaticTask {
            task: self.future,
            sender,
            poll_after: self.poll_after,
            hint: self.hint,
            priority: self.priority,
            executor: PhantomData,
            label: Some(self.label),
            task_id,
            cancellation: Some(cancellation),
        };
        (spawned_task, receiver)
    }

    /**
    Converts this task into one suitable for spawn_objsafe
    */
    pub fn into_objsafe(self) -> ObjSafeTask
    where
        N: ObserverNotified<F::Output> + Send,
        F::Output: Send + 'static + Unpin,
        F: Send + 'static,
    {
        let notifier = self.notifier.map(|n| {
            Box::new(ObserverNotifiedErased::new(n))
                as Box<dyn ObserverNotified<dyn Any + Send> + Send>
        });
        Task::new_objsafe(
            self.label,
            Box::new(async move { Box::new(self.future.await) as Box<dyn Any + Send + 'static> }),
            crate::task::Configuration::new(self.hint, self.priority, self.poll_after),
            notifier,
        )
    }

    /**
    Converts this task into one suitable for spawn_local_objsafe
    */
    pub fn into_objsafe_local(self) -> ObjSafeLocalTask
    where
        N: ObserverNotified<F::Output>,
        F::Output: 'static + Unpin,
        F: 'static,
    {
        let notifier = self.notifier.map(|n| {
            Box::new(ObserverNotifiedErasedLocal::new(n))
                as Box<dyn ObserverNotified<dyn Any + 'static>>
        });
        Task::new_objsafe_local(
            self.label,
            Box::new(async move { Box::new(self.future.await) as Box<dyn Any + 'static> }),
            crate::task::Configuration::new(self.hint, self.priority, self.poll_after),
            notifier,
        )
    }

    /// Converts this task into one suitable for spawn_static_objsafe
    pub fn into_objsafe_static(self) -> ObjSafeStaticTask
    where
        N: ObserverNotified<F::Output>,
        F::Output: 'static + Unpin,
        F: 'static,
    {
        let notifier = self.notifier.map(|n| {
            Box::new(ObserverNotifiedErasedLocal::new(n))
                as Box<dyn ObserverNotified<dyn Any + 'static>>
        });
        Task::new_objsafe_static(
            self.label,
            Box::new(async move { Box::new(self.future.await) as Box<dyn Any + 'static> }),
            crate::task::Configuration::new(self.hint, self.priority, self.poll_after),
            notifier,
        )
    }
}
