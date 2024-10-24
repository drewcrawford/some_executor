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

pub type Priority = priority::Priority;


use std::fmt::Debug;
use std::future::Future;
use crate::task::Task;
/*
Design notes.
Send is required because we often want to take this trait object and port it to another thread, etc.
Clone is required so that we can get copies for sending.
PartialEq could be used to compare runtimes, but I can't imagine anyone needs it
PartialOrd, Ord what does it mean?
Hash might make sense if we support eq but again, I can't imagine anyone needs it.
Debug might be nice but I dunno if it's needed.
I think all the rest are nonsense.


I think we don't want Sync, we want Send/Clone/mut semantics, and if the runtime can optimize it, it can.
*/
pub trait SomeExecutor: Send + Clone + 'static {
    /**
    Spawns a future onto the runtime.

    This function has few requirements on the underlying Future.

    # Parameters
    - `task`: The task to spawn.


    # Note

    `Send` and `'static` are generally required to move the future onto a new thread.

    # Implementation notes
    Implementations should generally ensure that a dlog-context is available to the future.
    */
    fn spawn<F: Future + Send + 'static>(&mut self, task: Task<F>);


    /**
    Spawns a future onto the runtime.

    Like [Self::spawn], but some implementors may have a fast path for the async context.

*/
    async fn spawn_async<F: Future + Send + 'static>(&mut self, task: Task<F>);




    /**
    Return an object-safe version of the runtime.

    This may be the same object, or it may be a wrapper that implements the objsafe API.
*/
    fn to_objsafe_runtime(self) -> Box<dyn SomeExecutorObjSafe>;
}

//sync is needed so that multiple threads can access e.g. a global shared reference.
pub trait SomeExecutorObjSafe: Send + Sync + Debug {
    /**
    Spawns a future onto the runtime.

    # Note

    This differs from [SomeExecutor::spawn_detached] in a few ways:
    1.  Takes a boxed future, since we can't have generic properties on an objsafe wrapper.  Implementations probably pin this with [Box::into_pin].
    2.  Takes an immutable reference.  Callers can't really clone `Box<dyn Trait>` to get a unique copy as this requires Size.  Implementors may wish to go with that or some other strategy (like an internal mutex, channel, etc.)
        We don't really want to specify "how it works", merely that this is a pattern clients want to do...
*/
    fn spawn_objsafe(&self, task: Task<Box<dyn Future<Output=()> + 'static + Send>>);
}

impl<A: SomeExecutor> From<A> for Box<dyn SomeExecutorObjSafe> {
    fn from(runtime: A) -> Self {
        runtime.to_objsafe_runtime()
    }
}


#[cfg(test)] mod tests {
    use crate::SomeExecutorObjSafe;

    #[test] fn test_is_objsafe() {
        fn is_objsafe(_obj: Box<dyn SomeExecutorObjSafe>) {}
    }
}