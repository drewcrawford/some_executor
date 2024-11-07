//SPDX-License-Identifier: MIT OR Apache-2.0

use std::any::Any;
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use crate::task::{InFlightTaskCancellation, TaskID};

#[derive(Debug,PartialEq,Eq,Hash,Clone)]
pub enum Observation<T> {
    /**
    The task is pending.
*/
    Pending,
    /**
    The task is finished.
*/
    Ready(T),
    /**
    The task was finished, but the value was already observed.
*/
    Done,
    /**
    The task was cancelled.
*/
    Cancelled,
}


#[derive(Debug)]
struct Shared<T> {
    //for now, we implement this with a mutex
    lock: std::sync::Mutex<Observation<T>>,
    /**
    Indicates the observer was dropped without detach.
    */
    observer_cancelled: AtomicBool,
    in_flight_task_cancellation: InFlightTaskCancellation,
}


/**
Observes information about a task.

Dropping the observer requests cancellation.

To detach instead, use [Observer::detach].

# Cancellation

Cancellation in some_executor is optimistic.  There are three types:

1.  some_executor itself guarantees that polls that occur logically after the cancellation will not be run.  So this "lightweight cancellation" is free and universal.
2.  The task may currently be in the process of being polled.  In this case, cancellation depends on how the task itself (that is, futures themselves, async code itself) reacts to cancellation through the `IS_CANCELLED` task local.
    Task support is sporadic and not guaranteed.
3.  The executor may support cancellation.  In this case, it may drop the future and not run it again.  This is not guaranteed.
*/
#[must_use]
#[derive(Debug)]
pub struct Observer<T,ENotifier:ExecutorNotified> {
    shared: Arc<Shared<T>>,
    task_id: TaskID,
    notifier: Option<ENotifier>,
    detached: bool,
}



impl<'executor, T,ENotifier: ExecutorNotified> Drop for Observer< T,ENotifier> {
    fn drop(&mut self) {
        if !self.detached {
            self.shared.observer_cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
            self.shared.in_flight_task_cancellation.cancel();
            self.notifier.take().map(|mut n| n.request_cancel());
        }
    }
}

/**
The sender side of an observer.  This side is held by the executor, and is used to send values to the observer.
*/
#[derive(Debug)]
pub(crate) struct ObserverSender<T,Notifier> {
    shared: Arc<Shared<T>>,
    pub(crate) notifier: Option<Notifier>
}

impl<T,Notifier> ObserverSender<T,Notifier> {
    pub(crate) fn send(&mut self, value: T) where Notifier: ObserverNotified<T> {
        self.notifier.as_mut().map(|n| n.notify(&value));
        let mut lock = self.shared.lock.lock().unwrap();
        match *lock {
            Observation::Pending => {
                *lock = Observation::Ready(value);
            },
            Observation::Ready(_) => {
                panic!("Observer already has a value");
            },
            Observation::Done => {
                panic!("Observer already completed");
            }
            Observation::Cancelled => {
                panic!("Observer cancelled");
            }
        }
    }

    pub(crate) fn observer_cancelled(&self) -> bool {
        self.shared.observer_cancelled.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl<T,Notifier> Drop for ObserverSender<T,Notifier> {
    fn drop(&mut self) {
        self.shared.in_flight_task_cancellation.cancel();
        let mut lock = self.shared.lock.lock().unwrap();
        match *lock {
            Observation::Pending => {
                *lock = Observation::Cancelled;
            },
            Observation::Ready(_) => {
                //nothing to do
            },
            Observation::Done => {
                //nothing to do
            }
            Observation::Cancelled => {
                panic!("Observer cancelled");
            }
        }
    }
}

impl<T,E: ExecutorNotified> Observer<T,E> {
    pub fn observe(&self) -> Observation<T> {
        let mut lock = self.shared.lock.lock().unwrap();
        match *lock {
            Observation::Pending => {
                Observation::Pending
            },
            Observation::Ready(..) => {
                let value = std::mem::replace(&mut *lock, Observation::Done);
                value
            },
            Observation::Done => {
                Observation::Done
            }
            Observation::Cancelled => {
                Observation::Cancelled
            }
        }
    }

    pub(crate) fn into_boxed_notifier(mut self) -> Observer< T,Box<dyn ExecutorNotified>> {
        Observer { shared: self.shared.clone(), task_id: self.task_id, notifier: self.notifier.take().map(|n| Box::new(n) as Box<dyn ExecutorNotified>), detached: self.detached }
    }
    /**
    Returns the task id of the task being observed.
*/
    pub fn task_id(&self) -> &TaskID {
        &self.task_id
    }

    /**
    Detaches from the active task, allowing it to continue running indefinitely.
    */
    pub fn detach(mut self) {
        self.notifier.take();
        self.detached = true;
    }
}

/**
Provides inline notifications to a user spawning a task, when the task completes.

The main difference between this and [crate::Observer] is that the observer can be polled to find
out if the task is done, while the notifier will be run inline when the task completes.

When the task is cancelled, the notifier will be dropped without running.  In this way one can
also receive inline notifications for cancellation.

# Design

The notifier is used inline in a future, (in a pinned context).  Accordingly there are two
possible designs:

1.  Use immutable references to the notifier.  But notifiers may want to have some mutable state,
    forcing them to figure out interior mutability and synchronization
2.  Require Unpin, allowing the type to be moved into the future.  This is the design we have chosen.

*/
pub trait ObserverNotified<T: ?Sized>: Unpin + 'static {

    /**
    This function will be run inline when the task completes.
*/
    fn notify(&mut self, value: &T);
}


/**
A trait for executors to receive notifications.

Handling notifications is optional.  If your executor does not want to bother, pass `None` in place
of functions taking this type, and set the type to `std::convert::Infallible`.  This is a special type
that cannot be constructed, yet it has implementations of the traits required.
*/
pub trait ExecutorNotified: 'static {
    /**
    This function is called when the user requests the task be cancelled.

    It is not required that executors handle this, but it may provide some efficiency.

    */
    fn request_cancel(&mut self);
}


impl<T> ObserverNotified<T> for Infallible {
    fn notify(&mut self, _value: &T) {
        panic!("NoNotified should not be used");
    }
}
//support unboxing
impl ObserverNotified<Box<(dyn std::any::Any + 'static)>> for Box<dyn ObserverNotified<(dyn std::any::Any + 'static)>> {
    fn notify(&mut self, value: &Box<(dyn Any + 'static)>) {
        let r = Box::as_mut(self);
        r.notify(value);
    }
}

//I guess a few ways?

impl ObserverNotified<Box<(dyn Any + Send + 'static)>> for Box<dyn ObserverNotified<(dyn Any + Send + 'static)> + Send> {
    fn notify(&mut self, value: &Box<(dyn Any + Send + 'static)>) {
        let r = Box::as_mut(self);
        r.notify(value);
    }
}

impl<'executor> ExecutorNotified for Infallible {
    fn request_cancel(&mut self) {
        panic!("NoNotified should not be used");
    }
}

pub(crate) fn observer_channel<'enotifier, R,ONotifier,ENotifier: ExecutorNotified>(observer_notify: Option<ONotifier>, executor_notify: Option<ENotifier>, task_cancellation: InFlightTaskCancellation, task_id: TaskID) -> (ObserverSender<R,ONotifier>, Observer< R,ENotifier>) {
    let shared = Arc::new(Shared { lock: std::sync::Mutex::new(Observation::Pending), observer_cancelled: AtomicBool::new(false), in_flight_task_cancellation: task_cancellation });
    (ObserverSender {shared: shared.clone(), notifier: observer_notify}, Observer {shared, task_id, notifier: executor_notify, detached: false})
}


/**
Allow a `Box<dyn ExecutorNotified>` to be used as an ExecutorNotified directly.

The implementation proceeds by dyanmic dispatch.
*/
impl ExecutorNotified for Box<dyn ExecutorNotified + '_> {
    fn request_cancel(&mut self) {
        (**self).request_cancel();
    }
}

/*
I don't really get why we need both of these... but we do!
 */
impl<'executor> ExecutorNotified for Box<dyn ExecutorNotified + Send> {
    fn request_cancel(&mut self) {
        (**self).request_cancel();
    }
}





/*
boilerplates

Observer - avoid copy/clone, Eq, Hash, default (channel), from/into, asref/asmut, deref, etc.

Observation - we want clone, Eq, Hash.
default is not obvious to me – could be pending but idk
we could support from based on the value
 */
impl <T> From<T> for  Observation<T> {
    fn from(value: T) -> Self {
        Observation::Ready(value)
    }
}

impl <T> From<Observation<T>> for Option<T> {
    fn from(value: Observation<T>) -> Self {
        match value {
            Observation::Ready(v) => Some(v),
            _ => None
        }
    }
}

#[cfg(test)] mod tests {
    use crate::observer::{ExecutorNotified, Observer};

    #[test] fn test_send() {

        /* observer can send when the underlying value can */
        #[allow(unused)]
        fn ex< 'executor, T: Send,E: ExecutorNotified + Send>(_observer: Observer<T,E>) {
            fn assert_send<T: Send>() {}
            assert_send::<Observer<T,E>>();
        }
    }
    #[test] fn test_unpin() {
        /* observer can unpin */
        #[allow(unused)]
        fn ex<'executor, T,E: ExecutorNotified + Unpin>(_observer: Observer<T,E>) {
            fn assert_unpin<T: Unpin>() {}
            assert_unpin::<Observer<T,E>>();
        }
    }
}