//SPDX-License-Identifier: MIT OR Apache-2.0
/*!

# some_executor

Rust made the terrible mistake of not having a batteries-included async executor.  And worse: there is
not even a standard trait (interface) that executors ought to implement.

There are a lot of opinions about what 'standard async rust' ought to be.  This crate is my opinion.

This crate does 3 simple jobs for 3 simple customers:

1.  For those *spawning tasks*, the crate provides a simple, obvious API for spawning tasks onto an unknown executor.
2.  For those *implementing executors*, this crate provides a simple, obvious API to receive tasks and execute them with few restrictions but many hints.
3.  For those *writing async code*, this crate provides a variety of nice abstractions that are portable across executors and do not depend on tokio.

# For those spawning tasks

Here are your options:

1.  The [SomeExecutorExt] trait provides an interface to spawn onto an executor.  You can take it as a generic argument.
2.  The [LocalExecutorExt] trait provides an interface to spawn onto a thread-local executor.  This is useful in case you have a future that is `!Send`.  You can take it as a generic argument.
3.  The object-safe versions, [SomeExecutor] and [SomeLocalExecutor], in case you want to store them in a struct by erasing their type.  This has the usual tradeoffs around boxing types.
4.  You can spawn onto the "current" executor, at task level [current_executor] or thread level [thread_executor].  This is useful in case you don't want to take an executor as an argument, but your caller probably has one, and you can borrow that.
5.  You can spawn onto a program-wide [global_executor].  This is useful in case you don't want to take it as an argument, you aren't sure what your caller is doing (for example you might be handling a signal), and you nonetheless want to spawn a task.

Altogether these options are quite flexible and cover many usecases, without overly burdening any side of the API.

Spawning a task is as simple as calling `spawn` on any of the executor types.  Then you get an [Observer] object that you can use to get the results of the task, if interested, or cancel the task.

# For those implementing executors

Here are your options:
1.  Implement the [SomeExecutorExt] trait.  This supports a wide variety of callers and patterns.
2.  If your executor is local to a thread, implement the [LocalExecutorExt] trait.  This type can spawn futures that are `!Send`.
3.  Optionally, respond to notifications by implementing the [ExecutorNotified] trait.  This is optional, but can provide some efficiency.


# For those writing async code

Mostly, write the code you want to write.  But here are some benefits you can get from this crate:

1.  If you need to spawn tasks from your async code, see above.
2.  The crate adds the [task_local] macro, which is comparable to `thread_local` or tokio's version.  It provides a way to store data that is local to the task.
3.  The provides various task locals, such as [task::TASK_ID] and [task::TASK_LABEL], which are useful for debugging and logging information about the current task.
4.  The crate propagates some locals, such as [task::TASK_PRIORITY], which can be used to provide useful downstream information about how the task is executing.
5.  The crate provides the [task::IS_CANCELLED] local, which can be used to check if the task has been cancelled.  This may provide some efficiency for cancellation.


# Development status

This interface is unstable and may change.
*/

pub mod task;
pub mod global_executor;
pub mod hint;
pub mod context;
pub mod observer;
pub mod thread_executor;
pub mod current_executor;

pub type Priority = priority::Priority;

use std::any::Any;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use crate::observer::{ExecutorNotified, NoNotified, Observer, ObserverNotified};
use crate::task::Task;
/*
Design notes.
Send is required because we often want to take this trait object and port it to another thread, etc.
Sync is required to have a global executor.
Clone is required so that we can get copies for sending.
PartialEq could be used to compare runtimes, but I can't imagine anyone needs it
PartialOrd, Ord what does it mean?
Hash might make sense if we support eq but again, I can't imagine anyone needs it.
Debug
I think all the rest are nonsense.
*/
/**
A trait targeting 'some' executor.

Code targeting this trait can spawn tasks on an executor without knowing which executor it is.

If possible, use the [SomeExecutorExt] trait instead.  But this trait is useful if you need an objsafe trait.
*/
pub trait SomeExecutor: Send + 'static + Sync {
    type ExecutorNotifier: ExecutorNotified;
    /**
    Spawns a future onto the runtime.

    # Parameters
    - `task`: The task to spawn.


    # Note

    `Send` and `'static` are generally required to move the future onto a new thread.

    # Implementation notes
    Implementations should generally ensure that a dlog-context is available to the future.
    */
    fn spawn<F: Future + Send + 'static, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F,Notifier>) -> Observer<F::Output,Self::ExecutorNotifier> where Self: Sized, F::Output: Send;


    /**
    Spawns a future onto the runtime.

    Like [Self::spawn], but some implementors may have a fast path for the async context.
*/
    fn spawn_async<F: Future + Send + 'static,Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F,Notifier>) -> impl Future<Output=Observer<F::Output,Self::ExecutorNotifier>> + Send + 'static where Self: Sized,F::Output: Send;

    /**
    Spawns a future onto the runtime.

    # Note

    This differs from [SomeExecutor::spawn] in that we take a boxed future, since we can't have generic fn.  Implementations probably pin this with [Box::into_pin].
    */
    fn spawn_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any + 'static + Send>> + 'static + Send>>,Box<DynONotifier>>) -> Observer<Box<dyn Any + 'static + Send>, Box<dyn ExecutorNotified + 'static + Send>>;

    /**
    Clones the executor.

    The returned value will spawn tasks onto the same executor.
*/
    fn clone_box(&self) -> Box<DynExecutor>;

    /**
    Produces an executor notifier.
*/
    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier>;
}

/**
A non-objsafe descendant of [SomeExecutor].

This trait provides a more ergonomic interface, but is not object-safe.
*/
pub trait SomeExecutorExt: SomeExecutor + Clone {

}

/**
A trait for executors that can spawn tasks onto the local thread.
*/
pub trait SomeLocalExecutor {
    type ExecutorNotifier: ExecutorNotified;
    /**
    Spawns a future onto the runtime.

    # Parameters
    - `task`: The task to spawn.

    */
    fn spawn_local<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F,Notifier>) -> Observer<F::Output,Self::ExecutorNotifier> where Self: Sized;

    /**
    Spawns a future onto the runtime.

    Like [Self::spawn], but some implementors may have a fast path for the async context.
    */
    fn spawn_local_async<F: Future ,Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F,Notifier>) -> impl Future<Output=Observer< F::Output, Self::ExecutorNotifier>> where Self: Sized;

    /**
    Spawns a future onto the runtime.

    # Note

    This differs from [SomeExecutor::spawn] in that we take a boxed future, since we can't have generic fn.  Implementations probably pin this with [Box::into_pin].
    */
    fn spawn_local_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>,Box<DynONotifier>>) -> Observer<Box<dyn Any>, Box<dyn ExecutorNotified>>;

    fn spawn_local_objsafe_async(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>,Box<DynONotifier>>) -> Box<dyn Future<Output=Observer<Box<dyn Any>, Box<dyn ExecutorNotified>>>>;


    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier>;

}

/**
The appropriate type for a dynamically-dispatched executor.
*/
pub type DynExecutor = dyn SomeExecutor<ExecutorNotifier = NoNotified>;


impl Debug for DynExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DynExecutor")
    }
}

impl Debug for AnyLocalExecutor<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DynLocalExecutor")
    }
}

pub type DynONotifier = dyn ObserverNotified<Box<dyn Any + Send>>;

/**
A non-objsafe descendant of [SomeLocalExecutor].

This trait provides a more ergonomic interface, but is not object-safe.

*/
pub trait LocalExecutorExt<'tasks>: SomeLocalExecutor + Clone {

}


struct SomeLocalExecutorErasingNotifier<'a, UnderlyingExecutor: SomeLocalExecutor + ?Sized > {
    executor: &'a mut UnderlyingExecutor,
}

impl<'executor, UnderlyingExecutor: SomeLocalExecutor> SomeLocalExecutor for SomeLocalExecutorErasingNotifier<'executor, UnderlyingExecutor> {
    type ExecutorNotifier = Box<dyn ExecutorNotified>;

    fn spawn_local<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F,Notifier>) -> Observer<F::Output,Self::ExecutorNotifier> where Self: Sized
    {
        todo!()
    }

    fn spawn_local_async<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F, Notifier>) -> impl Future<Output=Observer< F::Output, Self::ExecutorNotifier>>
    where
        Self: Sized
    {
        async {
            todo!()
        }
    }

    fn spawn_local_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<DynONotifier>>) -> Observer<Box<dyn Any>, Box<dyn ExecutorNotified>> {
        todo!()
    }

    fn spawn_local_objsafe_async(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<DynONotifier>>) -> Box<dyn Future<Output=Observer<Box<dyn Any>, Box<dyn ExecutorNotified>>>> {
        todo!()
    }

    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        todo!()
    }
}



pub struct AnyLocalExecutor<'underlying> {
    executor: Box<dyn SomeLocalExecutor<ExecutorNotifier=Box<dyn ExecutorNotified>>+ 'underlying>
}

impl<'a> AnyLocalExecutor<'a> {
    pub fn new<E: SomeLocalExecutor>(executor: &'a mut E) -> Self {
        AnyLocalExecutor {
            executor: Box::new(SomeLocalExecutorErasingNotifier { executor })
        }
    }
}
impl<'executor> SomeLocalExecutor for AnyLocalExecutor<'executor> {
    type ExecutorNotifier = Box<dyn ExecutorNotified>;

    fn spawn_local<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F,Notifier>) -> Observer<F::Output,Self::ExecutorNotifier> where Self: Sized
    {
        todo!()
    }

    fn spawn_local_async<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F, Notifier>) -> impl Future<Output=Observer<F::Output, Self::ExecutorNotifier>>
    where
        Self: Sized
    {
        async { todo!() }

    }

    fn spawn_local_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<DynONotifier>>) -> Observer<Box<dyn Any>, Box<dyn ExecutorNotified>> {
        self.executor.spawn_local_objsafe(task)
    }

    fn spawn_local_objsafe_async(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<DynONotifier>>) -> Box<dyn Future<Output=Observer<Box<dyn Any>, Box<dyn ExecutorNotified>>>> {
        todo!()
    }

    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        self.executor.executor_notifier().map(|x| Box::new(x) as Box<dyn ExecutorNotified>)
    }
}


#[cfg(test)] mod tests {
    use crate::{DynExecutor};

    #[test] fn test_is_objsafe() {
        #[allow(unused)]
        fn is_objsafe(_obj: &DynExecutor) {}
    }
}