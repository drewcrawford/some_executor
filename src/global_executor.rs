//SPDX-License-Identifier: MIT OR Apache-2.0
use std::sync::OnceLock;
use crate::{DynExecutor};

static GLOBAL_RUNTIME: OnceLock<Box<DynExecutor>> = OnceLock::new();

/**
Accesses an executor that is available for the global / arbitrary lifetime.

# Preconditions

The runtime must have been initialized with `set_global_runtime`.
# example
```
use some_executor::global_executor::global_executor;
let e = global_executor(|e| e.map(|e| e.clone_box()));
```
*/
pub fn global_executor<R>(c: impl FnOnce(Option<&DynExecutor>) -> R) -> R {
    let e = GLOBAL_RUNTIME.get();
    c(e.map(|e| &**e))
}

/**
Sets the global executor to this value.
Values that reference the global_runtime after this will see the new value.
*/
pub fn set_global_executor(runtime: Box<DynExecutor>) {
    GLOBAL_RUNTIME.set(runtime).expect("Global runtime already set");
}

#[cfg(test)] mod tests {
    use std::any::Any;
    use crate::global_executor::{global_executor};
    use crate::task::{ConfigurationBuilder, Task};

    #[test] fn global_pattern() {
        #[allow(unused)]
        fn dont_execute_just_compile() {
            let mut runtime = global_executor(|e| e.unwrap().clone_box());
            let configuration = ConfigurationBuilder::new().build();
            //get a Box<dyn Any>

            let task = Task::new_objsafe("test".into(), Box::new(async {
                Box::new(()) as Box<dyn Any + Send + 'static>
                // todo!()
            }), configuration, None);
            runtime.spawn_objsafe(task);
        }
    }
}