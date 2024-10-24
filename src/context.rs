/*!
Provides a context type that can be used to query the current task.
*/

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};



#[macro_export]
macro_rules! task_local {
// empty (base case for the recursion)
    () => {};

    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty; $($rest:tt)*) => (
        $crate::__task_local_inner!($(#[$attr])* $vis $name, $t);
        $crate::task_local!($($rest)*);
    );

    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty) => (
        $crate::__task_local_inner!($(#[$attr])* $vis $name, $t);
    );

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

pub struct LocalKey<T: 'static>(pub /* do not use */ std::thread::LocalKey<RefCell<Option<T>>>);

/// A future that sets a value `T` of a task local for the future `F` during
/// its execution.
///
/// The value of the task-local must be `'static` and will be dropped on the
/// completion of the future.
///
/// Created by the function [`LocalKey::scope`](self::LocalKey::scope).
pub(crate) struct TaskLocalFuture<V: 'static,F> {
    slot: Option<V>,
    local_key: &'static LocalKey<V>,
    future: F,
}
impl<V,F> Future for TaskLocalFuture<V,F> where V: Unpin, F: Future {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        //destructure
        let (future, slot, local_key) = unsafe {
            let this = self.get_unchecked_mut();
            let future = Pin::new_unchecked(&mut this.future);
            let slot = Pin::new_unchecked(&mut this.slot);
            let local_key = Pin::new_unchecked(&mut this.local_key);
            (future,slot,local_key)
        };

        let mut_slot = Pin::get_mut(slot);
        let value = mut_slot.take().expect("No value in slot");
        let old_value = local_key.0.replace(Some(value));
        assert!(old_value.is_none(), "Task-local already set");
        let r = future.poll(cx);
        //put value back in slot
        let value = local_key.0.replace(None).expect("No value in slot");
        mut_slot.replace(value);
        r
    }
}

impl<T: 'static> LocalKey<T> {
    /**
    Sets the value of the task-local for the duration of the future `F`.
*/
    pub fn scope<F>(&'static self, value: T, f: F) -> TaskLocalFuture<T, F>
    where
        F: Future,
    {
        TaskLocalFuture {
            slot: Some(value),
            local_key: self,
            future: f,
        }
    }

    /**
    Accesses the underlying task-local inside the closure.

    If the task-local is not set, the closure receives `None`.
*/
    pub fn with<F, R>(&'static self, f: F) -> R
    where
        F: FnOnce(Option<&T>) -> R {
        self.0.with(|slot| {
            let value = slot.borrow();
            f(value.as_ref())
        })
    }

    /**
    Accesses the underlying task-local inside the closure, mutably.

    If the task-local is not set, the closure receives `None`.
*/
    pub fn with_mut<F, R>(&'static self, f: F) -> R
    where
        F: FnOnce(Option<&mut T>) -> R {
        self.0.with(|slot| {
            let mut value = slot.borrow_mut();
            f(value.as_mut())
        })
    }

    /**
    Returns a copy of the contained value.
*/
    pub fn get(&'static self) -> T where T: Copy {
        self.0.with(|slot| {
            let value = slot.borrow();
            value.expect("Task-local not set").clone()
        })
    }

    /**
    Sets or initializes the contained value.

    Unlike the other methods, this will not run the lazy initializer of the thread local. Instead, it will be directly initialized with the given value if it wasn’t initialized yet.
*/
    pub fn set(&'static self, value: T) {
        self.0.set(Some(value))
    }

    /**
    Replaces the contained value, returning the old value.

    This will lazily initialize the value if this thread has not referenced this key yet.
*/
    fn replace(&'static self, value: T) -> T {
        self.0.replace(Some(value)).expect("Task-local not set")
    }







}

#[cfg(test)] mod tests {
    #[test] fn local() {
        task_local! {
            static FOO: u32;
        }

    }
}