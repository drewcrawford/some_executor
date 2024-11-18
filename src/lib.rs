//SPDX-License-Identifier: MIT OR Apache-2.0
/*!

# some_executor

Rust made the terrible mistake of not having an async executor in std.  And worse: there is no
trait for executors to implement, nor a useful API for users to expect.  The result is everyone has to write
their code to a specific executor, and it's always tokio.  But tokio has too many drawbacks to make
it the universal choice, and the other executors are too cumbersome to be practical.  As a result,
async rust is stuck in limbo.

There are many proposals to fix this.  This one's mine.  Here's how it works:

**If you want to execute futures**, this crate provides a simple, obvious trait to spawn your future onto "some" executor.
The rich APIs are thoughtfully designed to support typical applications, be fast, compatible with many executors, and future-proof.  For example, you
can take an executor as a generic argument, giving the compiler the opportunity to specialize your code for the specific executor.
Or, you can spawn a task onto a global executor via dynamic dispatch.  You can provide rich scheduling information that
can be used by the executor to prioritize tasks. You can do all this in a modular and futureproof way.

**If you want to implement an executor**, this crate provides a simple, obvious trait to receive futures
and execute them, and plug into the ecosystem.  Moreover, advanced features like cancellation are
implemented for you, so you get them for free and can focus on the core logic of your executor.

**If you want to write async code**, this crate provides a **standard, robust featureset** that (in my opinion) is
 table-stakes for writing async rust in 2024. This includes cancellation, task locals, priorities, and much more.
These features are portable and dependable across any executor.

Here are deeper dives on each topic.

# For those spawning tasks

some_executor provides many different API options for many usecases.

1.  The [SomeExecutorExt] trait provides an interface to spawn onto an executor.  You can take it as a generic argument, specializing your code/types against
    the executor you want to use.
2.  The [LocalExecutorExt] trait provides the analogous interface for local executors (for your futures which are `!Send`).
3.  The object-safe versions, [SomeExecutor] and [SomeLocalExecutor], are for when you want to store your executor in a struct by erasing their type.  This has the usual tradeoffs around boxing types.
4.  You can spawn onto the "current" executor, at task level [current_executor] or thread level [thread_executor].  This is useful in case you don't want to take an executor as an argument, but your caller probably has one, and you can borrow that.
5.  You can spawn onto a program-wide [global_executor].  This is useful in case you don't want to take it as an argument, you aren't sure what your caller is doing (for example you might be handling a signal), and you nonetheless want to spawn a task.

Spawning a task is as simple as calling `spawn` on any of the executor types.  Then you get an [TypedObserver] object that you can use to get the results of the task, if interested, or cancel the task.

## Reference executors:

* [test_executors](https://sealedabstract.com/code/test_executors) provides a set of toy executors good enough for unit tests.
* [some_local_executor](https://sealedabstract.com/code/some_local_executor) provides a local executor that runs its task on the current thread, and can also receive tasks from other threads.
* A reference thread-per-core executor is planned.

# For those implementing executors

Here are your APIs:
1.  Implement the [SomeExecutorExt] trait.  This supports a wide variety of callers and patterns.
2.  Alternatively, or in addition, if your executor is local to a thread, implement the [LocalExecutorExt] trait.  This type can spawn futures that are `!Send`.
3.  Optionally, respond to notifications by implementing the [ExecutorNotified] trait.  This is optional, but can provide some efficiency.

The main gotcha of this API is that you must wait to poll tasks until after [Task::poll_after].  You can
accomplish this any way you like, such as suspending the task, sleeping, etc.  For more details, see the documentation.

# For those writing async code

Mostly, write the code you want to write.  But here are some benefits you can get from this crate:

1.  If you need to spawn [Task]s from your async code, see above.
2.  The crate adds the [task_local] macro, which is comparable to `thread_local` or tokio's version.  It provides a way to store data that is local to the task.
3.  The provides various particular task locals, such as [task::TASK_ID] and [task::TASK_LABEL], which are useful for debugging and logging information about the current task.
4.  The crate propagates some locals, such as [task::TASK_PRIORITY], which can be used to provide useful downstream information about how the task is executing.
5.  The crate provides the [task::IS_CANCELLED] local, which can be used to check if the task has been cancelled.  This allows you to return early and avoid unnecessary work.
6.  In the future, support for task groups and parent-child cancellation may be added.

# Alternative to `executor-trait`

One way to understand this crate is as an alternative to the [executor-trait](https://crates.io/crates/executor-trait/) project.  While I like it a lot,
here's why I made this instead:

1.  To support futures with output types that are not `()`.
2.  To avoid boxing futures in cases where it isn't really necessary.
3.  To provide hints and priorities to the executor.
4.  To support task locals and other features that are useful for async code.
5.  To support task cancellation much more robustly.

Philosophically, the difference is that `executor-trait` ships the lowest-common denominator API that all executors can support.  While this
crate ships the **highest-common denominator API that all async code can use**, together with **polyfills and fallbacks so all executors
can use them** even if they don't support them natively.  The result is rich, fast, easy, and portable async rust.

It is straightforward to implement the API of this crate in terms of `executor-trait`, as well as the reverse.  So it is possible
to use both projects together.

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
mod local;
mod sys;

pub use sys::Instant as Instant;

pub type Priority = priority::Priority;

use std::any::Any;
use std::convert::Infallible;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use crate::observer::{ExecutorNotified, TypedObserver, ObserverNotified, Observer};
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
pub trait SomeExecutor: Send + Sync {
    type ExecutorNotifier: ExecutorNotified + Send;
    /**
    Spawns a future onto the runtime.

    # Parameters
    - `task`: The task to spawn.


    # Note

    `Send` and `'static` are generally required to move the future onto a new thread.

    # Implementation notes
    Implementations should generally ensure that a dlog-context is available to the future.
    */
    fn spawn<F: Future + Send + 'static, Notifier: ObserverNotified<F::Output> + Send>(&mut self, task: Task<F, Notifier>) -> impl Observer<Value=F::Output>
    where
        Self: Sized,
        F::Output: Send;


    /**
    Spawns a future onto the runtime.

    Like [Self::spawn], but some implementors may have a fast path for the async context.
    */
    fn spawn_async<F: Future + Send + 'static, Notifier: ObserverNotified<F::Output> + Send>(&mut self, task: Task<F, Notifier>) -> impl Future<Output=impl Observer<Value=F::Output>> + Send + 'static
    where
        Self: Sized,
        F::Output: Send;

    /**
    Spawns a future onto the runtime.

    # Note

    This differs from [SomeExecutor::spawn] in that we take a boxed future, since we can't have generic fn.  Implementations probably pin this with [Box::into_pin].
    */
    fn spawn_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any + 'static + Send>> + 'static + Send>>, Box<dyn ObserverNotified<dyn Any + Send> + Send>>) -> Box<dyn Observer<Value=dyn Any + Send>>;

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
pub trait SomeExecutorExt: SomeExecutor + Clone {}

/**
A trait for executors that can spawn tasks onto the local thread.
*/
pub trait SomeLocalExecutor<'future> {
    type ExecutorNotifier: ExecutorNotified;
    /**
    Spawns a future onto the runtime.

    # Parameters
    - `task`: The task to spawn.

    */
    fn spawn_local<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F, Notifier>) -> TypedObserver<F::Output, Self::ExecutorNotifier>
    where
        Self: Sized,
        F: 'future,
    /* I am a little uncertain whether this is really required */
        <F as Future>::Output: Unpin
    ;

    /**
    Spawns a future onto the runtime.

    Like [Self::spawn_local], but some implementors may have a fast path for the async context.
    */
    fn spawn_local_async<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, task: Task<F, Notifier>) -> impl Future<Output=TypedObserver<F::Output, Self::ExecutorNotifier>>
    where
        Self: Sized,
        F: 'future;

    /**
    Spawns a future onto the runtime.

    # Note

    This differs from [SomeExecutor::spawn] in that we take a boxed future, since we can't have generic fn.  Implementations probably pin this with [Box::into_pin].
    */
    fn spawn_local_objsafe(&mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> TypedObserver<Box<dyn Any>, Box<dyn ExecutorNotified>>;

    fn spawn_local_objsafe_async<'s>(&'s mut self, task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Future<Output=TypedObserver<Box<dyn Any>, Box<dyn ExecutorNotified>>> + 's>;


    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier>;
}

/**
The appropriate type for a dynamically-dispatched executor.
*/
pub type DynExecutor = dyn SomeExecutor<ExecutorNotifier=Infallible>;


impl Debug for DynExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DynExecutor")
    }
}




/**
A non-objsafe descendant of [SomeLocalExecutor].

This trait provides a more ergonomic interface, but is not object-safe.

# Note

We don't support clone on LocalExecutor.  There are a few reasons:
1.  The typical case of thread-local executor may operate primarily on the principle of (mutable) references into stack memory.
    Accordingly, a clone may involve some kind of stack-heap transfer (can't really do that since Futures are pinned), or effectively
    requiring heap allocations from the start.  This is to be avoided.
2.  Similarly, the mentioned mutable references make it impossible to have two mutable references to the same executor.
3.  The SomeExecutor abstraction works more like a channel, where senders can be cloned.  In practice, local executors want to implement
    that – if at all – by bolting such a channel onto their executor with some kind of adapter tool.  This allows the overhead to be
    avoided in cases it isn't used.
*/
pub trait LocalExecutorExt<'tasks>: SomeLocalExecutor<'tasks> {}


impl<'future> SomeLocalExecutor<'future> for Infallible {
    type ExecutorNotifier = Infallible;

    fn spawn_local<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, _task: Task<F, Notifier>) -> TypedObserver<F::Output, Self::ExecutorNotifier>
    where
        Self: Sized,
        F: 'future,
        <F as Future>::Output: Unpin
    {
        unimplemented!()
    }

    fn spawn_local_async<F: Future, Notifier: ObserverNotified<F::Output>>(&mut self, _task: Task<F, Notifier>) -> impl Future<Output=TypedObserver<F::Output, Self::ExecutorNotifier>>
    where
        Self: Sized,
        F: 'future
    {
        async {unimplemented!()}
    }

    fn spawn_local_objsafe(&mut self, _task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> TypedObserver<Box<dyn Any>, Box<dyn ExecutorNotified>> {
        unimplemented!()
    }

    fn spawn_local_objsafe_async<'s>(&'s mut self, _task: Task<Pin<Box<dyn Future<Output=Box<dyn Any>>>>, Box<dyn ObserverNotified<(dyn Any + 'static)>>>) -> Box<dyn Future<Output=TypedObserver<Box<dyn Any>, Box<dyn ExecutorNotified>>> + 's> {
        unimplemented!()
    }

    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        unimplemented!()
    }
}


#[cfg(test)]
mod tests {
    use crate::{DynExecutor};
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_is_objsafe() {
        #[allow(unused)]
        fn is_objsafe(_obj: &DynExecutor) {}
    }
}