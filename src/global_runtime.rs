use std::sync::OnceLock;
use crate::{DynExecutor};

static GLOBAL_RUNTIME: OnceLock<Box<DynExecutor>> = OnceLock::new();

/**
Accesses a runtime that is available for the global / arbitrary lifetime.

# Preconditions

The runtime must have been initialized with `set_global_runtime`.
*/
pub fn global_runtime() -> &'static DynExecutor {
    GLOBAL_RUNTIME.get().expect("Global runtime not initialized").as_ref()
}

/**
Sets the global runtime to this value.
Values that reference the global_runtime after this will see the new value.
*/
pub fn set_global_runtime(runtime: Box<DynExecutor>) {
    GLOBAL_RUNTIME.set(runtime).unwrap_or_else(|_| panic!("Global runtime already initialized"));
}

#[cfg(test)] mod tests {
    use std::any::Any;
    use crate::global_runtime::global_runtime;
    use crate::task::{ConfigurationBuilder, Task};

    #[test] fn global_pattern() {
        #[allow(unused)]
        fn dont_execute_just_compile() {
            let mut runtime = global_runtime().clone_box();
            let configuration = ConfigurationBuilder::new().build();
            //get a Box<dyn Any>

            let task = Task::new_objsafe("test".into(), Box::new(async {
                Box::new(()) as Box<dyn Any>
                // todo!()
            }), configuration, None);
            runtime.spawn_objsafe(task);
        }
    }
}