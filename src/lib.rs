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

use std::future::Future;

pub trait ARuntime {
    /**
    Spawns a future onto the runtime.
*/
    fn spawn_detached<F: Future + Send>(&self, priority: priority::Priority, runtime_hint: RuntimeHint, f: F);
}

#[cfg(test)] mod tests {

}