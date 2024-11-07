# some_executor

!`logo`(art/logo.png)

Rust made the terrible mistake of not having an async executor in std.  And worse: there is no
trait for executors to implement, nor a useful API for users to expect.  The result is everyone has to write
their code to a specific executor, and it's always tokio.  But tokio has too many drawbacks to make
it the universal choice, and the other executors are too cumbersome to be practical.  As a result,
async rust is stuck in limbo.

There are many proposals to fix this.  This one's mine.  Here's how it works:

**If you want to execute futures**, this crate provides a simple, obvious trait to spawn your future onto "some" executor.
The extensive APIs are thoughtfully designed to support typical applications, be fast, compatible with many executors, and future-proof.  For example, you
can take an executor as a generic argument, giving the compiler the opportunity to specialize your code for the specific executor.
Or, you can spawn a task onto a global executor via dynamic dispatch.  You can provide rich scheduling information that
can be used by the executor to prioritize tasks. Or, you can run the same code on a very simple executor that does not do
any scheduling.  You can write code today that is compatible with executors that don't exist yet.

**If you want to implement an executor**, this crate provides a simple, obvious trait to receive futures and execute them.  Connecting
to the API is super easy, giving you access to all users of the API, and also free implementations of many high-level features people want.
And deeply integrating with the API allows customization and optimization for your specific executor.

**If you want to write async code**, this crate provides a **standard, robust featureset** that (in my opinion) is
table-stakes for async-rust-2024.  This includes task-local variables, cancellation, task names, scheduling priorities,
and more.  More importantly this crate ships a *reference implementation that is compatible with all executors*, so you can depend
on them being there, at the same time that specific executors can optimize them to be even better.

# For those spawning tasks

some_executor provides many different APIs for many usecases.

1.  The `SomeExecutorExt` trait provides an interface to spawn onto an executor.  You can take it as a generic argument, specializing your code.
2.  The `LocalExecutorExt` trait provides the analagous interface for local executors, for your futures which are `!Send`.
3.  The object-safe versions, `SomeExecutor` and `SomeLocalExecutor`, in case you want to store your executor in a struct by erasing their type.  This has the usual tradeoffs around boxing types.
4.  You can spawn onto the "current" executor, at task level `current_executor` or thread level `thread_executor`.  This is useful in case you don't want to take an executor as an argument, but your caller probably has one, and you can borrow that.
5.  You can spawn onto a program-wide `global_executor`.  This is useful in case you don't want to take it as an argument, you aren't sure what your caller is doing (for example you might be handling a signal), and you nonetheless want to spawn a task.

Spawning a task is as simple as calling `spawn` on any of the executor types.  Then you get an `Observer` object that you can use to get the results of the task, if interested, or cancel the task.

## Reference implementations:

* `test_executors`(https://sealedabstract.com/code/test_executors) provides a set of toy executors good enough for unit tests.
* `some_local_executor`(https://sealedabstract.com/code/some_local_executor) provides a local executor that runs its task on the current thread, and can also receive tasks from other threads.
* A reference thread-per-core executor is planned.

# For those implementing executors

Here are your options:
1.  Implement the `SomeExecutorExt` trait.  This supports a wide variety of callers and patterns.
2.  Alternatively, or in addition, if your executor is local to a thread, implement the `LocalExecutorExt` trait.  This type can spawn futures that are `!Send`.
3.  Optionally, respond to notifications by implementing the `ExecutorNotified` trait.  This is optional, but can provide some efficiency.


# For those writing async code

Mostly, write the code you want to write.  But here are some benefits you can get from this crate:

1.  If you need to spawn `Task`s from your async code, see above.
2.  The crate adds the `task_local` macro, which is comparable to `thread_local` or tokio's version.  It provides a way to store data that is local to the task.
3.  The provides various particular task locals, such as `task::TASK_ID` and `task::TASK_LABEL`, which are useful for debugging and logging information about the current task.
4.  The crate propagates some locals, such as `task::TASK_PRIORITY`, which can be used to provide useful downstream information about how the task is executing.
5.  The crate provides the `task::IS_CANCELLED` local, which can be used to check if the task has been cancelled.  This may provide some efficiency for cancellation.
6.  In the future, support for task groups and parent-child cancellation may be added.

# Development status

This interface is unstable and may change.