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
pub trait ARuntime: Send + Clone + ARuntimeObjSafe {
    /**
    Spawns a future onto the runtime.

    # Note

    `Send` and `'static` are generally required to move the future onto a new thread.
    */
    fn spawn_detached<F: Future + Send + 'static>(&mut self, priority: priority::Priority, runtime_hint: RuntimeHint, f: F);
}

pub trait ARuntimeObjSafe: Send + Sync + Debug {
    fn spawn_detached_objsafe(&mut self, priority: priority::Priority, runtime_hint: RuntimeHint, f: Box<dyn Future<Output=()> + Send + 'static>);
}




pub trait ARuntimeExt: ARuntime {
    /**they say we can't have async closures, but what about closures that produce a future?
*/
    fn spawn_detached_closure<F: FnOnce() -> Fut,Fut: Future + Send + 'static>(&mut self, priority: priority::Priority, runtime_hint: RuntimeHint, f: F) {
        self.spawn_detached(priority, runtime_hint, f());
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
    use crate::ARuntimeObjSafe;

    #[test] fn obj_safe() {
        fn assert_obj_safe<T>(_foo: &dyn ARuntimeObjSafe) {}
    }
}