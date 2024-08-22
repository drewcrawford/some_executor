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

    # Note

    `Send` and `'static` are generally required to move the future onto a new thread.
*/
    fn spawn_detached<F: Future + Send + 'static>(&mut self, priority: priority::Priority, runtime_hint: RuntimeHint, f: F);
}

#[cfg(test)] mod tests {

}