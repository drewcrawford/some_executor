use std::sync::OnceLock;
use crate::SomeExecutorObjSafe;

static GLOBAL_RUNTIME: OnceLock<Box<dyn SomeExecutorObjSafe>> = OnceLock::new();

/**
Accesses a runtime that is available for the global / arbitrary lifetime.

# Preconditions

The runtime must have been initialized with `set_global_runtime`.
*/
pub fn global_runtime() -> &'static dyn SomeExecutorObjSafe {
    GLOBAL_RUNTIME.get().expect("Global runtime not initialized").as_ref()
}

/**
Sets the global runtime to this value.
Values that reference the global_runtime after this will see the new value.
*/
pub fn set_global_runtime(runtime: Box<dyn SomeExecutorObjSafe>) {
    GLOBAL_RUNTIME.set(runtime).expect("didn't set global runtime");
}