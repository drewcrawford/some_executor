use std::sync::OnceLock;
use crate::SomeExecutor;

static GLOBAL_RUNTIME: OnceLock<Box<dyn SomeExecutor>> = OnceLock::new();

/**
Accesses a runtime that is available for the global / arbitrary lifetime.

# Preconditions

The runtime must have been initialized with `set_global_runtime`.
*/
pub fn global_runtime() -> &'static dyn SomeExecutor {
    GLOBAL_RUNTIME.get().expect("Global runtime not initialized").as_ref()
}

/**
Sets the global runtime to this value.
Values that reference the global_runtime after this will see the new value.
*/
pub fn set_global_runtime(runtime: Box<dyn SomeExecutor>) {
    GLOBAL_RUNTIME.set(runtime).unwrap_or_else(|_| panic!("Global runtime already initialized"));
}

#[cfg(test)] mod tests {
    use crate::global_runtime::global_runtime;
    use crate::task::{ConfigurationBuilder, Task};

    #[test] fn global_pattern() {
        fn dont_execute_just_compile() {
            let mut runtime = global_runtime().clone_box();
            runtime.spawn_objsafe(Task::new("test", Box::new(async { }), ConfigurationBuilder::new().build()));
        }
    }
}