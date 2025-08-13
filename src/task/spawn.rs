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
    /// 1. A [`spawned::SpawnedTask`] that can be polled by the executor
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
    /// 1. A [`spawned::SpawnedLocalTask`] that can be polled by the executor
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
    /// 1. A [`spawned::SpawnedStaticTask`] that can be polled by the executor
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

    /// Spawns the task onto an executor using object-safe type erasure.
    ///
    /// This method enables spawning tasks when the executor type is not known at compile time,
    /// using dynamic dispatch instead of static dispatch. The spawner receives type-erased
    /// handles while the executor still works with concrete types.
    ///
    /// # Object Safety
    ///
    /// "Object-safe" here means the method can be called through a trait object (`dyn SomeExecutor`).
    /// This requires:
    /// - No generic parameters in the trait method signature
    /// - Type erasure of the future and its output type
    ///
    /// # Type Erasure
    ///
    /// The method erases types for the spawner but not the executor:
    /// - **Spawner side**: Receives boxed, type-erased observer
    /// - **Executor side**: Still works with the concrete future type
    ///
    /// # Arguments
    ///
    /// * `executor` - The executor that will run this task
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// 1. A [`spawned::SpawnedTask`] with the concrete future type
    /// 2. A [`TypedObserver`] with a boxed executor notifier
    ///
    /// # Examples
    ///
    /// ```
    /// # use some_executor::task::{Task, Configuration};
    /// # use some_executor::SomeExecutor;
    /// # use some_executor::observer::{Observer, ObserverNotified, TypedObserver};
    /// # use std::any::Any;
    /// # use std::pin::Pin;
    /// # use std::future::Future;
    /// # use std::convert::Infallible;
    /// #
    /// # // Mock executor for testing
    /// # #[derive(Debug, Clone)]
    /// # struct TestExecutor;
    /// # impl SomeExecutor for TestExecutor {
    /// #     type ExecutorNotifier = Infallible;
    /// #     fn spawn<F, N>(&mut self, task: Task<F, N>) -> impl Observer<Value = F::Output>
    /// #     where F: Future + Send + 'static, N: ObserverNotified<F::Output> + Send + 'static, F::Output: Send + 'static
    /// #     { todo!() as TypedObserver<F::Output, Infallible> }
    /// #     fn spawn_async<F, N>(&mut self, task: Task<F, N>) -> impl Future<Output = impl Observer<Value = F::Output>>
    /// #     where F: Future + Send + 'static, N: ObserverNotified<F::Output> + Send + 'static, F::Output: Send + 'static
    /// #     { async { todo!() as TypedObserver<F::Output, Infallible> } }
    /// #     fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> { None }
    /// #     fn clone_box(&self) -> Box<dyn SomeExecutor<ExecutorNotifier = Self::ExecutorNotifier>> { Box::new(self.clone()) }
    /// #     fn spawn_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any + Send>> + Send>>, Box<dyn ObserverNotified<dyn Any + Send> + Send>>) -> Box<dyn Observer<Value=Box<dyn Any + Send>, Output=some_executor::observer::FinishedObservation<Box<dyn Any + Send>>> + Send> { todo!() }
    /// #     fn spawn_objsafe_async<'s>(&'s mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any + Send>> + Send>>, Box<dyn ObserverNotified<dyn Any + Send> + Send>>) -> Box<dyn Future<Output=Box<dyn Observer<Value=Box<dyn Any + Send>, Output=some_executor::observer::FinishedObservation<Box<dyn Any + Send>>> + Send>> + 's> { Box::new(async { todo!() }) }
    /// # }
    /// #
    /// let mut executor = TestExecutor;
    /// let task = Task::without_notifications(
    ///     "objsafe-task".to_string(),
    ///     Configuration::default(),
    ///     async { "result" }
    /// );
    ///
    /// let (spawned, observer) = task.spawn_objsafe(&mut executor);
    /// // Observer has type-erased executor notifier
    /// ```
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

    /// Spawns the task onto a local executor using object-safe type erasure.
    ///
    /// Similar to [`spawn_objsafe`](Self::spawn_objsafe) but for local executors that
    /// don't require `Send`. This enables spawning tasks on thread-local executors
    /// when the executor type is not known at compile time.
    ///
    /// # Object Safety
    ///
    /// This method provides the same type erasure benefits as `spawn_objsafe` but
    /// for local executors. The spawner receives type-erased handles while the
    /// executor continues to work with concrete types.
    ///
    /// # Arguments
    ///
    /// * `executor` - The local executor that will run this task
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// 1. A [`spawned::SpawnedLocalTask`] with the concrete future type
    /// 2. A [`TypedObserver`] with a boxed executor notifier
    ///
    /// # Examples
    ///
    /// ```
    /// # use some_executor::task::{Task, Configuration};
    /// # use some_executor::SomeLocalExecutor;
    /// # use some_executor::observer::{Observer, ObserverNotified, FinishedObservation, TypedObserver};
    /// # use std::rc::Rc;
    /// # use std::any::Any;
    /// # use std::pin::Pin;
    /// # use std::future::Future;
    /// # use std::convert::Infallible;
    /// #
    /// # // Mock local executor for testing
    /// # #[derive(Debug)]
    /// # struct TestLocalExecutor;
    /// # impl<'a> SomeLocalExecutor<'a> for TestLocalExecutor {
    /// #     type ExecutorNotifier = Infallible;
    /// #     fn spawn_local<F: Future, N: ObserverNotified<F::Output>>(&mut self, task: Task<F, N>) -> impl Observer<Value = F::Output>
    /// #     where F: 'a, F::Output: 'static
    /// #     { todo!() as TypedObserver<F::Output, Infallible> }
    /// #     fn spawn_local_async<F: Future, N: ObserverNotified<F::Output>>(&mut self, task: Task<F, N>) -> impl Future<Output = impl Observer<Value = F::Output>>
    /// #     where F: 'a, F::Output: 'static
    /// #     { async { todo!() as TypedObserver<F::Output, Infallible> } }
    /// #     fn spawn_local_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output = Box<dyn Any>>>>, Box<dyn ObserverNotified<dyn Any + 'static>>>) -> Box<dyn Observer<Value = Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>>
    /// #     { todo!() }
    /// #     fn spawn_local_objsafe_async<'s>(&'s mut self, task: Task<Pin<Box<dyn Future<Output = Box<dyn Any>>>>, Box<dyn ObserverNotified<dyn Any + 'static>>>) -> Box<dyn Future<Output = Box<dyn Observer<Value = Box<dyn Any>, Output = FinishedObservation<Box<dyn Any>>>>> + 's>
    /// #     { Box::new(async { todo!() }) }
    /// #     fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> { None }
    /// # }
    /// #
    /// let mut executor = TestLocalExecutor;
    /// // Works with !Send types
    /// let data = Rc::new(42);
    /// let data_clone = data.clone();
    /// let task = Task::without_notifications(
    ///     "local-objsafe".to_string(),
    ///     Configuration::default(),
    ///     async move { *data_clone }
    /// );
    ///
    /// let (spawned, observer) = task.spawn_local_objsafe(&mut executor);
    /// ```
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
    /// 1. A [`spawned::SpawnedStaticTask`] that can be polled by the executor
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

    /// Converts this task into an object-safe task for type-erased spawning.
    ///
    /// This method transforms a concrete task into one that can be spawned using
    /// [`spawn_objsafe`](crate::SomeExecutor::spawn_objsafe) on executors accessed
    /// through trait objects.
    ///
    /// # Requirements
    ///
    /// - Future must be `Send + 'static`
    /// - Future output must be `Send + 'static + Unpin`
    /// - Notifier must be `Send`
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration};
    ///
    /// let task = Task::without_notifications(
    ///     "my-task".to_string(),
    ///     Configuration::default(),
    ///     async { 42 }
    /// );
    ///
    /// // Convert to object-safe task
    /// let objsafe_task = task.into_objsafe();
    /// // Now can be spawned on `dyn SomeExecutor`
    /// ```
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

    /// Converts this task into an object-safe task for local type-erased spawning.
    ///
    /// This method transforms a concrete task into one that can be spawned using
    /// [`spawn_local_objsafe`](crate::SomeLocalExecutor::spawn_local_objsafe) on
    /// local executors accessed through trait objects.
    ///
    /// # Requirements
    ///
    /// - Future must be `'static` (but not necessarily `Send`)
    /// - Future output must be `'static + Unpin`
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration};
    /// use std::rc::Rc;
    ///
    /// // Works with !Send types
    /// let data = Rc::new(vec![1, 2, 3]);
    /// let data_clone = data.clone();
    /// let task = Task::without_notifications(
    ///     "local-task".to_string(),
    ///     Configuration::default(),
    ///     async move { data_clone.len() }
    /// );
    ///
    /// // Convert to object-safe local task
    /// let objsafe_task = task.into_objsafe_local();
    /// // Now can be spawned on `dyn SomeLocalExecutor`
    /// ```
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
