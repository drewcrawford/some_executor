/*!
aruntime is a set of traits that define an async runtime.

You can imagine it to be an *abstract* runtime (in the sense that it defines a set of methods that a runtime must implement, but does not provide a concrete implementation).

You can also imagine it to be a-runtime (that is, not a runtime).
*/

/**
A type that describes the expected runtime characteristics of the future.
*/
#[non_exhaustive]
pub enum RuntimeHint {
    /**
    We don't know anything about the future.
    */
    Unknown,
    /**
    The future is expected to spend most of its time yielded.
    */
    IO,

    /**
    The future is expected to spend most of its time computing.
    */
    CPU,
}

use std::fmt::Debug;
use std::future::Future;
use std::sync::OnceLock;
/*
Design notes.
Send is required because we often want to take this trait object and port it to another thread, etc.
Clone is required so that we can get copies for sending.
PartialEq could be used to compare runtimes, but I can't imagine anyone needs it
PartialOrd, Ord what does it mean?
Hash might make sense if we support eq but again, I can't imagine anyone needs it.
Debug might be nice but I dunno if it's needed.
I think all the rest are nonsense.


I think we don't want Sync, we want Send/Clone semantics, and if the runtime can optimize it, it can.
*/
pub trait ARuntime: Send + Clone {
    /**
    Spawns a future onto the runtime.

    # Parameters
    - `label`: A label for the future.  This is useful for debugging.
    - `priority`: The priority of the future.  This is useful for scheduling.
    - `runtime_hint`: A hint about the runtime characteristics of the future.  This is useful for scheduling.
    - `f`: The future to spawn.

    # Note

    `Send` and `'static` are generally required to move the future onto a new thread.

    # Implementation notes
    Implementations should generally ensure that a dlog-context is available to the future.
    */
    fn spawn_detached<F: Future + Send + 'static>(&mut self, label: &'static str, priority: priority::Priority, runtime_hint: RuntimeHint, f: F);

    /**
    Like [spawn_detached], but some implementors may have a fast path for the async context.
*/
    fn spawn_detached_async<F: Future + Send + 'static>(&mut self, label: &'static str, priority: priority::Priority, runtime_hint: RuntimeHint, f: F) -> impl Future<Output=()>;

    /**
    Spawns a future onto the runtime after a certain time.

    Conforming runtimes guarantee that the future is spawned 'not before' `time`  That is, a caller can expect `assert!(Instant::now() >= time)` when the future is spawned.

    However, runtimes are not required to spawn the future exactly at `time`, and it may be later under load.  Runtimes are expected to use reasonable best efforts to spawn the future close to `time`,
    but "reasonable" is in the eye of the implementor and subject to the constraints of the runtime.

    # Parameters
    - `label`: A label for the future.  This is useful for debugging.
    - `priority`: The priority of the future.  This is useful for scheduling.
    - `runtime_hint`: A hint about the runtime characteristics of the future.  This is useful for scheduling.
    - `time`: The time to spawn the future.
    - `f`: The future to spawn.

*/
    fn spawn_after<F: Future + Send + 'static>(&mut self, label: &'static str, priority: priority::Priority, runtime_hint: RuntimeHint, time: std::time::Instant, f: F);

    /**
    Like [spawn_after], but some implementors may have a fast path for the async context.
*/
    fn spawn_after_async<F: Future + Send + 'static>(&mut self, label: &'static str, priority: priority::Priority, runtime_hint: RuntimeHint, time: std::time::Instant, f: F) -> impl Future<Output=()>;

    /**
    Return an object-safe version of the runtime.

    This may be the same object, or it may be a wrapper that implements the objsafe API.
*/
    fn to_objsafe_runtime(self) -> Box<dyn ARuntimeObjSafe>;
}

//sync is needed so that multiple threads can access e.g. a global shared reference.
pub trait ARuntimeObjSafe: Send + Sync + Debug {
    /**
    Spawns a future onto the runtime.

# Note
    This differs from [spawn_detached] in a few ways:
    1.  Takes a boxed future, since we can't have generic properties on an objsafe wrapper.  Implementations probably pin this with [Box::into_pin].
    2.  Takes an immutable reference.  Callers can't really clone Box<dyn Trait> to get a unique copy as this requires Size.  Implementors may wish to go with that or some other strategy (like an internal mutex, channel, etc.)
        We don't really want to specify "how it works", merely that this is a pattern clients want to do...

# Implementation notes
    Implementations should generally ensure that a dlog-context is available to the future.
*/
    fn spawn_detached_objsafe(&self, label: &'static str, priority: priority::Priority, runtime_hint: RuntimeHint, f: Box<dyn Future<Output=()> + Send + 'static>);


}

impl<A: ARuntime> From<A> for Box<dyn ARuntimeObjSafe> {
    fn from(runtime: A) -> Self {
        runtime.to_objsafe_runtime()
    }
}



pub trait ARuntimeExt: ARuntime {
    /**they say we can't have async closures, but what about closures that produce a future?
*/
    fn spawn_detached_closure<F: FnOnce() -> Fut,Fut: Future + Send + 'static>(&mut self, label: &'static str, priority: priority::Priority, runtime_hint: RuntimeHint, f: F) {
        self.spawn_detached(label, priority, runtime_hint, f());
    }
}

impl<Runtime: ARuntime> ARuntimeExt for Runtime {}

static GLOBAL_RUNTIME: OnceLock<Box<dyn ARuntimeObjSafe>> = OnceLock::new();

/**
Accesses a runtime that is available for the global / arbitrary lifetime.

# Preconditions

The runtime must have been initialized with `set_global_runtime`.
*/
pub fn global_runtime() -> &'static dyn ARuntimeObjSafe {
    GLOBAL_RUNTIME.get().expect("Global runtime not initialized").as_ref()
}

/**
Sets the global runtime to this value.
Values that reference the global_runtime after this will see the new value.
*/
pub fn set_global_runtime(runtime: Box<dyn ARuntimeObjSafe>) {
    GLOBAL_RUNTIME.set(runtime).expect("didn't set global runtime");
}


#[cfg(test)] mod tests {

}