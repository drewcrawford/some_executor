// SPDX-License-Identifier: MIT OR Apache-2.0

//! Future implementations for task-local storage.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::context::{LocalKey, LocalKeyImmutable};

/// A future that sets a value `T` of a task local for the future `F` during
/// its execution.
///
/// This future wraps another future and ensures that a task-local value is
/// available during the wrapped future's execution. The task-local value is
/// stored in a slot and swapped into thread-local storage each time the future
/// is polled, then swapped back out when polling completes.
///
/// The value of the task-local must be `'static` and will be dropped on the
/// completion of the future.
///
/// # Implementation Details
///
/// The future maintains the task-local value in a `slot` field. During each
/// poll:
/// 1. The value is moved from the slot into thread-local storage
/// 2. The wrapped future is polled
/// 3. The value is moved back from thread-local storage to the slot
///
/// This ensures the value is available to the future and any code it calls,
/// while keeping it safe from concurrent access.
///
/// Created by the function [`LocalKey::scope`](crate::context::LocalKey::scope).
#[derive(Debug)]
pub(crate) struct TaskLocalFuture<V: 'static, F> {
    pub(crate) slot: Option<V>,
    pub(crate) local_key: &'static LocalKey<V>,
    pub(crate) future: F,
}

/// A future that sets an immutable task-local value for the duration of
/// another future's execution.
///
/// Similar to [`TaskLocalFuture`], but for immutable task-locals that cannot
/// be modified once set within a scope. This provides stronger guarantees about
/// the stability of configuration values during task execution.
///
/// # Implementation Details
///
/// Works identically to `TaskLocalFuture` in terms of the slot/swap mechanism,
/// but the associated `LocalKeyImmutable` type prevents mutation of the value
/// while it's in scope.
///
/// Created by the function [`LocalKeyImmutable::scope`](crate::context::LocalKeyImmutable::scope).
#[derive(Debug)]
pub(crate) struct TaskLocalImmutableFuture<V: 'static, F> {
    pub(crate) slot: Option<V>,
    pub(crate) local_key: &'static LocalKeyImmutable<V>,
    pub(crate) future: F,
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

/// Implementation of `Future` for `TaskLocalFuture`.
///
/// This implementation handles the mechanics of swapping task-local values
/// in and out of thread-local storage during polling.
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

/// Implementation of `Future` for `TaskLocalImmutableFuture`.
///
/// This implementation handles the mechanics of swapping immutable task-local
/// values in and out of thread-local storage during polling.
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
