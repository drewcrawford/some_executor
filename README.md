# some_executor

!`logo`(art/logo.png)

Rust made the terrible mistake of not having a batteries-included async executor.  And worse: there is
not even a standard trait (interface) that executors ought to implement.

There are a lot of opinions about what 'standard async rust' ought to be.  This crate is my opinion.

This crate does 3 simple jobs for 3 simple customers:

1.  For those *spawning tasks*, the crate provides a simple, obvious API for spawning tasks onto an unknown executor.
2.  For those *implementing executors*, this crate provides a simple, obvious API to receive tasks and execute them with few restrictions but many hints.
3.  For those *writing async code*, this crate provides a variety of nice abstractions that are portable across executors and do not depend on tokio.

# For those spawning tasks

Here are your options:

1.  The `SomeExecutorExt` trait provides an interface to spawn onto an executor.  You can take it as a generic argument.
2.  The `LocalExecutorExt` trait provides an interface to spawn onto a thread-local executor.  This is useful in case you have a future that is `!Send`.  You can take it as a generic argument.
3.  The object-safe versions, `SomeExecutor` and `SomeLocalExecutor`, in case you want to store them in a struct by erasing their type.  This has the usual tradeoffs around boxing types.
4.  You can spawn onto the "current" executor, at task level `current_executor` or thread level `thread_executor`.  This is useful in case you don't want to take an executor as an argument, but your caller probably has one, and you can borrow that.
5.  You can spawn onto a program-wide `global_executor`.  This is useful in case you don't want to take it as an argument, you aren't sure what your caller is doing (for example you might be handling a signal), and you nonetheless want to spawn a task.

Altogether these options are quite flexible and cover many usecases, without overly burdening any side of the API.

Spawning a task is as simple as calling `spawn` on any of the executor types.  Then you get an `Observer` object that you can use to get the results of the task, if interested, or cancel the task.

# For those implementing executors

Here are your options:
1.  Implement the `SomeExecutorExt` trait.  This supports a wide variety of callers and patterns.
2.  If your executor is local to a thread, implement the `LocalExecutorExt` trait.  This type can spawn futures that are `!Send`.
3.  Optionally, respond to notifications by implementing the `ExecutorNotified` trait.  This is optional, but can provide some efficiency.


# For those writing async code

Mostly, write the code you want to write.  But here are some benefits you can get from this crate:

1.  If you need to spawn tasks from your async code, see above.
2.  The crate adds the `task_local` macro, which is comparable to `thread_local` or tokio's version.  It provides a way to store data that is local to the task.
3.  The provides various task locals, such as `task::TASK_ID` and `task::TASK_LABEL`, which are useful for debugging and logging information about the current task.
4.  The crate propagates some locals, such as `task::TASK_PRIORITY`, which can be used to provide useful downstream information about how the task is executing.
5.  The crate provides the `task::IS_CANCELLED` local, which can be used to check if the task has been cancelled.  This may provide some efficiency for cancellation.



# Development status

This interface is unstable and may change.