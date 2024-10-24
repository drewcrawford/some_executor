/*!
Provides a context type that can be used to query the current task.
*/

use std::cell::RefCell;

pub struct TaskContext {

}

impl Default for TaskContext {
    fn default() -> Self {
        TaskContext {

        }
    }
}

thread_local! {
    static CONTEXT: RefCell<Option<TaskContext>> = RefCell::new(None);
}

impl TaskContext {
    /**
    Provides access to the current context, within a closure.

    If the current context is not available, the argument to the closure is None.
    */
    pub fn current_mut<F,R>(f: F) -> R where F: FnOnce(&mut Option<TaskContext>) -> R {
        CONTEXT.with_borrow_mut(f)
    }

    /**
    Provides access to the current context, within a closure.

    If the current context is not available, the argument to the closure is None.
    */
    pub fn current<F,R>(f: F) -> R where F: FnOnce(&Option<TaskContext>) -> R {
        CONTEXT.with_borrow(f)
    }
}