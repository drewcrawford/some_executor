//SPDX-License-Identifier: MIT OR Apache-2.0
/*!

# some_executor

Rust made the terrible mistake of not having a batteries-included async executor.  And worse: there is
not even a standard trait (interface) that executors ought to implement.

The result is that libraries that want to be executor-agnostic and coded against 'some executor' are in a rough place.  Often they wind
up coding 'to [tokio](https://tokio.rs)' directly, which is a fine runtime but maybe downstream crates
wanted a different one.  Or, they can code to an interface like [agnostic](https://docs.rs/agnostic/latest/agnostic/index.html),
but that is suspiciously large and somehow depends on tokio, which motivates the 'lite version' [agnostic-lite](https://crates.io/crates/agnostic-lite),
which somehow still has 50 lines of features for tokio, smol, async-io, sleep, time, etc.

Anyway, this crate's one and *only* job is to define an obvious and minimal API trait (façade) that libraries can consume to
spawn their task on 'some' async executor.  Implementing that API for any given executor is trivial, however to keep the
crate small this exercise is left to that executor, a third-party crate consuming both APIs, or to the reader.

The crate implements both a trait suitable for generic code with zero-cost abstraction, and an object-safe trait suitable for
dynamic dispatch.

This crate also defines a 'global' executor, suitable for 'get' by library code and 'set' by application code.

# Development status

This interface is unstable and may change.
*/

mod task;
mod global_runtime;
mod hint;
mod context;

pub type Priority = priority::Priority;


use std::future::Future;
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
    /**
    Spawns a future onto the runtime.

    # Parameters
    - `task`: The task to spawn.


    # Note

    `Send` and `'static` are generally required to move the future onto a new thread.

    # Implementation notes
    Implementations should generally ensure that a dlog-context is available to the future.
    */
    fn spawn<F: Future + Send + 'static>(&mut self, task: Task<F>) where Self: Sized;


    /**
    Spawns a future onto the runtime.

    Like [Self::spawn], but some implementors may have a fast path for the async context.
*/
    async fn spawn_async<F: Future + Send + 'static>(&mut self, task: Task<F>) where Self: Sized;

    /**
    Spawns a future onto the runtime.

    # Note

    This differs from [SomeExecutor::spawn] in that we take a boxed future, since we can't have generic properties.  Implementations probably pin this with [Box::into_pin].
    */
    fn spawn_objsafe(&mut self, task: Task<Box<dyn Future<Output=()> + 'static + Send>>);

    /**
    Clones the executor.

    The returned value will spawn tasks onto the same executor.
*/
    fn clone_box(&self) -> Box<dyn SomeExecutor>;
}

/**
A non-objsafe descendant of [SomeExecutor].

This trait provides a more ergonomic interface, but is not object-safe.
*/
pub trait SomeExecutorExt: SomeExecutor + Clone {

}



#[cfg(test)] mod tests {
    use crate::SomeExecutor;

    #[test] fn test_is_objsafe() {
        fn is_objsafe(_obj: &dyn SomeExecutor) {}
    }
}