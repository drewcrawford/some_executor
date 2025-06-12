//SPDX-License-Identifier: MIT OR Apache-2.0
/*!
Provides a context type that can be used to query the current task.
*/

use std::cell::RefCell;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

/**
Defines a task-local key.

This is a thread-local value that is unique to the task that sets it.  It is similar to `thread_local!` but is scoped to tasks.


# Example

```
some_executor::task_local! {
    static FOO: u32;
    //immutable task-locals cannot be set
    static const BAR: u32;
}

async fn demo() {
    let foo = FOO.scope(42, async {
        assert_eq!(FOO.get(), 42);
        FOO.set(43);
        assert_eq!(FOO.get(), 43);

        let bar = BAR.scope(44, async {
            assert_eq!(BAR.get(), 44);
        }).await;
        //BAR.get() outside the scope will panic.
    }).await;
}
```

Task-local defines either a mutable [LocalKey] or an immutable [LocalKeyImmutable].  The difference is that the immutable version cannot be set.


*/
#[macro_export]
macro_rules! task_local {
// empty (base case for the recursion)
    () => {};

    ($(#[$attr:meta])* $vis:vis static const $name:ident: $t:ty; $($rest:tt)*) => (
        $crate::__task_local_inner_immutable!($(#[$attr])* $vis $name, $t);
        $crate::task_local!($($rest)*);
    );

    ($(#[$attr:meta])* $vis:vis static const $name:ident: $t:ty) => (
        $crate::__task_local_inner_immutable!($(#[$attr])* $vis $name, $t);
    );

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

            $crate::context::LocalKey::new(__KEY)
        };
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __task_local_inner_immutable {
    ($(#[$attr:meta])* $vis:vis $name:ident, $t:ty) => {
        $(#[$attr])*
        $vis static $name: $crate::context::LocalKeyImmutable<$t> = {
            std::thread_local! {
                static __KEY: std::cell::RefCell<Option<$t>> = const { std::cell::RefCell::new(None) };
            }

            $crate::context::LocalKeyImmutable::new(__KEY)
        };
    };
}

/**
Defines a task-local value.
*/
#[derive(Debug)]
pub struct LocalKey<T: 'static>(std::thread::LocalKey<RefCell<Option<T>>>);

impl<T: 'static> LocalKey<T> {
    #[doc(hidden)]
    pub const fn new(key: std::thread::LocalKey<RefCell<Option<T>>>) -> Self {
        LocalKey(key)
    }
}

/**
Defines an immutable task-local value.

# A word on immutability

Immutable task-locals cannot be mutated while they are in scope.  However, the meaning of this is nonintuitive.

Reading the value of the task-local (such as with [LocalKeyImmutable::get] or [LocalKeyImmutable::with]) can return different results
at different times, which may be surprising for a 'constant' value.  This is because the value is only constant for the duration of the scope,
while a future may execute across various scopes at various times.

The primary utility of this type is to forbid "downstream" modifications of the task-local.  For example,
if a task-local is set in a parent task, it may be desirable to prevent a child task from modifying it.
*/
#[derive(Debug)]
pub struct LocalKeyImmutable<T: 'static>(std::thread::LocalKey<RefCell<Option<T>>>);

impl<T: 'static> LocalKeyImmutable<T> {
    #[doc(hidden)]
    pub const fn new(key: std::thread::LocalKey<RefCell<Option<T>>>) -> Self {
        LocalKeyImmutable(key)
    }
}

/// A future that sets a value `T` of a task local for the future `F` during
/// its execution.
///
/// The value of the task-local must be `'static` and will be dropped on the
/// completion of the future.
///
/// Created by the function [`LocalKey::scope`](self::LocalKey::scope).
#[derive(Debug)]
pub(crate) struct TaskLocalFuture<V: 'static, F> {
    slot: Option<V>,
    local_key: &'static LocalKey<V>,
    future: F,
}

#[derive(Debug)]
pub(crate) struct TaskLocalImmutableFuture<V: 'static, F> {
    slot: Option<V>,
    local_key: &'static LocalKeyImmutable<V>,
    future: F,
}

impl<V, F> TaskLocalFuture<V, F> {
    //     /**
    //     Gets access to the underlying value.
    // */
    //
    //     pub(crate) fn get_val<R>(&self, closure:impl FnOnce(&V) -> R) -> R  {
    //         match self.slot {
    //             Some(ref value) => closure(value),
    //             None => self.local_key.with(|value| {
    //                 closure(value.expect("Value neither in slot nor in thread-local"))
    //             })
    //         }
    //     }
    //     /**
    //     Gets mutable access to the underlying value.
    // */
    //     pub(crate) fn get_val_mut<R>(&mut self, closure:impl FnOnce(&mut V) -> R) -> R  {
    //         match self.slot {
    //             Some(ref mut value) => closure(value),
    //             None => self.local_key.with_mut(|value| {
    //                 closure(value.expect("Value neither in slot nor in thread-local"))
    //             })
    //         }
    //     }

    // pub(crate) fn get_future(&self) -> &F {
    //     &self.future
    // }
    //
    // pub (crate) fn get_future_mut(&mut self) -> &mut F {
    //     &mut self.future
    // }
    //
    // pub(crate) fn into_future(self) -> F {
    //     self.future
    // }
}

impl<V, F> TaskLocalImmutableFuture<V, F> {
    // /**
    // Gets access to the underlying value.
    // */
    //
    // pub(crate) fn get_val<R>(&self, closure:impl FnOnce(&V) -> R) -> R  {
    //     match self.slot {
    //         Some(ref value) => closure(value),
    //         None => self.local_key.with(|value| {
    //             closure(value.expect("Value neither in slot nor in thread-local"))
    //         })
    //     }
    // }

    // pub(crate) fn get_future(&self) -> &F {
    //     &self.future
    // }

    // pub (crate) fn get_future_mut(&mut self) -> &mut F {
    //     &mut self.future
    // }

    // pub(crate) fn into_future(self) -> F {
    //     self.future
    // }
}

impl<V, F> Future for TaskLocalFuture<V, F>
where
    V: Unpin,
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        //destructure
        let (future, slot, local_key) = unsafe {
            let this = self.get_unchecked_mut();
            let future = Pin::new_unchecked(&mut this.future);
            let slot = Pin::new_unchecked(&mut this.slot);
            let local_key = Pin::new_unchecked(&mut this.local_key);
            (future, slot, local_key)
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

impl<V, F> Future for TaskLocalImmutableFuture<V, F>
where
    V: Unpin,
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        //destructure
        let (future, slot, local_key) = unsafe {
            let this = self.get_unchecked_mut();
            let future = Pin::new_unchecked(&mut this.future);
            let slot = Pin::new_unchecked(&mut this.slot);
            let local_key = Pin::new_unchecked(&mut this.local_key);
            (future, slot, local_key)
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
    pub(crate) fn scope_internal<F>(&'static self, value: T, f: F) -> TaskLocalFuture<T, F>
    where
        F: Future,
    {
        TaskLocalFuture {
            slot: Some(value),
            local_key: self,
            future: f,
        }
    }

    pub fn scope<F>(&'static self, value: T, f: F) -> impl Future<Output = F::Output>
    where
        F: Future,
        T: Unpin,
    {
        self.scope_internal(value, f)
    }

    /**
        Accesses the underlying task-local inside the closure.

        If the task-local is not set, the closure receives `None`.
    */
    pub fn with<F, R>(&'static self, f: F) -> R
    where
        F: FnOnce(Option<&T>) -> R,
    {
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
        F: FnOnce(Option<&mut T>) -> R,
    {
        self.0.with(|slot| {
            let mut value = slot.borrow_mut();
            f(value.as_mut())
        })
    }

    /**
        Returns a copy of the contained value.
    */
    pub fn get(&'static self) -> T
    where
        T: Copy,
    {
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
    pub fn replace(&'static self, value: T) -> T {
        self.0.replace(Some(value)).expect("Task-local not set")
    }
}

impl<T: 'static> LocalKeyImmutable<T> {
    /**
    Sets the value of the task-local for the duration of the future `F`.
    */
    pub(crate) fn scope_internal<F>(&'static self, value: T, f: F) -> TaskLocalImmutableFuture<T, F>
    where
        F: Future,
    {
        TaskLocalImmutableFuture {
            slot: Some(value),
            local_key: self,
            future: f,
        }
    }

    pub fn scope<F>(&'static self, value: T, f: F) -> impl Future<Output = F::Output>
    where
        F: Future,
        T: Unpin,
    {
        self.scope_internal(value, f)
    }

    /**
    Returns a copy of the contained value.
    */
    pub fn get(&'static self) -> T
    where
        T: Copy,
    {
        self.0.with(|slot| {
            let value = slot.borrow();
            value.expect("Task-local not set").clone()
        })
    }

    /**
    Accesses the underlying task-local inside the closure.

    If the task-local is not set, the closure receives `None`.
    */
    pub fn with<F, R>(&'static self, f: F) -> R
    where
        F: FnOnce(Option<&T>) -> R,
    {
        self.0.with(|slot| {
            let value = slot.borrow();
            f(value.as_ref())
        })
    }

    /**
    Accesses the underlying task-local inside the closure, mutably.

    # Safety
    This is unsafe because the type guarantees the type is immutable, but you are mutating it
    outside of the scope of the task-local.
    */
    pub(crate) unsafe fn with_mut<F, R>(&'static self, f: F) -> R
    where
        F: FnOnce(&mut Option<T>) -> R,
    {
        self.0.with(|slot| {
            let mut value = slot.borrow_mut();
            f(&mut value)
        })
    }
}
/*
boilerplates
LocalKey - underlying doesn't support clone.  This eliminates copy,eq,ord,default,etc.
from/into does not make a lot of sense, neither does asref/deref
Looks like send/sync/unpin ought to carry through
drop is sort of specious for static types
 */

#[cfg(test)]
mod tests {
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn local() {
        task_local! {
            #[allow(unused)]
            static FOO: u32;
            #[allow(unused)]
            static const BAR: u32;
        }
    }
}
