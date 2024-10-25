//SPDX-License-Identifier: MIT OR Apache-2.0
/*!

# some_executor

Rust made the terrible mistake of not having a batteries-included async executor.  And worse: there is
not even a standard trait (interface) that executors ought to implement.

There are a lot of opinions about what 'standard async rust' ought to be.  This crate is my opinion.

This crate does 3 simple jobs for 3 simple customers:

1.  For those *spawning tasks*, the crate provides a simple, obvious API for spawning tasks onto an unknown executor.
2.  For those *implementing executors*, this crate provides a simple, obvious API to receive tasks.
3.  For those *writing async code*, this crate provides a variety of nice abstractions that are portable across executors and do not depend on tokio.

# For those spawning tasks

Here are your options:

1.  The [SomeExecutorExt] trait provides an interface to spawn onto an executor.  You can take it as a generic argument.
2.  The [LocalExecutorExt] trait provides an interface to spawn onto a thread-local executor.  This is useful in case you have a future that is `!Send`.  You can take it as a generic argument.
3.  The object-safe versions, [SomeExecutor] and [LocalExecutor], in case you want to store them in a struct by erasing their type.  This has the usual tradeoffs around boxing types.
4.  Task-local versions of the above.

# Development status

This interface is unstable and may change.
*/

pub mod task;
pub mod global_runtime;
pub mod hint;
pub mod context;
pub mod observer;

pub type Priority = priority::Priority;

use std::any::Any;
use std::future::Future;
use std::pin::Pin;
use crate::observer::{ExecutorNotified, Observer, ObserverNotified};
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
    fn spawn<F: Future + Send + 'static, Notifier: Send + 'static>(&mut self, task: Task<F,Notifier>) -> Observer<F::Output,Self::ExecutorNotifier> where Self: Sized;


    /**
    Spawns a future onto the runtime.

    Like [Self::spawn], but some implementors may have a fast path for the async context.
*/
    fn spawn_async<F: Future + Send + 'static,Notifier: Send + 'static>(&mut self, task: Task<F,Notifier>) -> impl Future<Output=Observer<F::Output,Self::ExecutorNotifier>> + Send + 'static where Self: Sized;

    /**
    Spawns a future onto the runtime.

    # Note

    This differs from [SomeExecutor::spawn] in that we take a boxed future, since we can't have generic fn.  Implementations probably pin this with [Box::into_pin].
    */
    fn spawn_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>> + 'static + Send>>,Box<DynONotifier>>) -> Observer<Box<dyn Any>, Box<dyn ExecutorNotified>>;

    /**
    Clones the executor.

    The returned value will spawn tasks onto the same executor.
*/
    fn clone_box(&self) -> Box<dyn SomeExecutor<ExecutorNotifier = Box<dyn ExecutorNotified>>>;

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
pub trait LocalExecutor: SomeExecutor {
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
    fn spawn_local_async<F: Future,Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F,Notifier>) -> impl Future<Output=Observer<F::Output, Self::ExecutorNotifier>> + Send + 'static where Self: Sized;

    /**
    Spawns a future onto the runtime.

    # Note

    This differs from [SomeExecutor::spawn] in that we take a boxed future, since we can't have generic fn.  Implementations probably pin this with [Box::into_pin].
    */
    fn spawn_local_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>,Box<DynONotifier>>) -> Observer<Box<dyn Any>, Box<dyn ExecutorNotified>>;
}

/**
The appropriate type for a dynamically-dispatched executor.
*/
pub type DynExecutor = dyn SomeExecutor<ExecutorNotifier = Box<dyn ExecutorNotified>>;

pub type DynONotifier = dyn ObserverNotified<Box<dyn Any>>;

/**
A non-objsafe descendant of [LocalExecutor].

This trait provides a more ergonomic interface, but is not object-safe.

*/
pub trait LocalExecutorExt: LocalExecutor + Clone {

}



#[cfg(test)] mod tests {
    use crate::{DynExecutor};

    #[test] fn test_is_objsafe() {
        #[allow(unused)]
        fn is_objsafe(_obj: &DynExecutor) {}
    }
}