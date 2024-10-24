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
    pub(crate) fn current_mut<F,R>(f: F) -> R where F: FnOnce(&mut Option<TaskContext>) -> R {
        CONTEXT.with_borrow_mut(f)
    }

    /**
    Provides access to the current context, within a closure.

    If the current context is not available, the argument to the closure is None.
    */
    fn current<F,R>(f: F) -> R where F: FnOnce(&Option<TaskContext>) -> R {
        CONTEXT.with_borrow(f)
    }
}

#[macro_export]
macro_rules! task_local {
     // empty (base case for the recursion)
    () => {};

    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty; $($rest:tt)*) => {
        $crate::__task_local_inner!($(#[$attr])* $vis $name, $t);
        $crate::task_local!($($rest)*);
    };

    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty) => {
        $crate::__task_local_inner!($(#[$attr])* $vis $name, $t);
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! __task_local_inner {
    ($(#[$attr:meta])* $vis:vis $name:ident, $t:ty) => {
        $(#[$attr])*
        $vis static $name: $crate::context::LocalKey<$t> = {
            std::thread_local! {
                static __KEY: std::cell::RefCell<Option<$t>> = const { std::cell::RefCell::new(None) };
            }

            $crate::context::LocalKey(__KEY)
        };
    };
}

struct LocalKey<T: 'static>(std::thread::LocalKey<RefCell<Option<T>>>);

impl<T: 'static> LocalKey<T> {
    fn with<F, R>(&'static self, f: F) -> R
    where
        F: FnOnce(&T) -> R {
        self.0.with(|slot| {
            f(&slot.borrow().as_ref().expect("task-local value not set"))
        })
    }
}

#[cfg(test)] mod tests {
    #[test] fn local() {
        task_local! {
            static FOO: u32;
            static BAR: String;
        }

        FOO.with(|f| {
            assert_eq!(*f, 0);
        });
    }
}