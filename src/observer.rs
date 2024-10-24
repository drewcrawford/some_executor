use std::sync::Arc;
use crate::task::TaskID;

#[derive(Debug)]
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
}


/**
Observes information about a task.
*/
#[derive(Debug)]
pub struct Observer<T,ENotifier: ExecutorNotified> {
    shared: Arc<Shared<T>>,
    task_id: TaskID,
    notifier: Option<ENotifier>,
}

impl<T,ENotifier: ExecutorNotified> Drop for Observer<T,ENotifier> {
    fn drop(&mut self) {
        self.notifier.as_mut().map(|n| n.request_cancel());
    }
}

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
}

impl<T,Notifier> Drop for ObserverSender<T,Notifier> {
    fn drop(&mut self) {
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
    /**
    Returns the task id of the task being observed.
*/
    pub fn task_id(&self) -> &TaskID {
        &self.task_id
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
pub trait ObserverNotified<T>: Unpin {

    /**
    This function will be run inline when the task completes.
*/
    fn notify(&mut self, value: &T);
}

pub trait ExecutorNotified {
    /**
    This function is called when the user requests the task be cancelled.
    */
    fn request_cancel(&mut self);
}

pub struct NoNotified;
impl<T> ObserverNotified<T> for NoNotified {
    fn notify(&mut self, _value: &T) {
        panic!("NoNotified should not be used");
    }
}

impl ExecutorNotified for NoNotified {
    fn request_cancel(&mut self) {
        panic!("NoNotified should not be used");
    }
}

pub(crate) fn observer_channel<R,ONotifier,ENotifier: ExecutorNotified>(observer_notify: Option<ONotifier>, executor_notify: Option<ENotifier>, task_id: TaskID) -> (ObserverSender<R,ONotifier>, Observer<R,ENotifier>) {
    let shared = Arc::new(Shared { lock: std::sync::Mutex::new(Observation::Pending) });
    (ObserverSender {shared: shared.clone(), notifier: observer_notify}, Observer {shared, task_id, notifier: executor_notify})
}


/**
Allow a Box<dyn ExecutorNotified> to be used as an ExecutorNotified directly.

The implementation proceeds by dyanmic dispatch.
*/
impl ExecutorNotified for Box<dyn ExecutorNotified> {
    fn request_cancel(&mut self) {
        (**self).request_cancel();
    }
}


/*
boilerplates

Observer - avoid copy/clone, Eq, Hash, default (channel), from/into, asref/asmut, deref, etc.
 */

#[cfg(test)] mod tests {
    use crate::observer::{ExecutorNotified, Observer};

    #[test] fn test_send() {

        /* observer can send when the underlying value can */
        #[allow(unused)]
        fn ex<T: Send,E: ExecutorNotified + Send>(_observer: Observer<T,E>) {
            fn assert_send<T: Send>() {}
            assert_send::<Observer<T,E>>();
        }
    }
    #[test] fn test_unpin() {
        /* observer can unpin */
        #[allow(unused)]
        fn ex<T,E: ExecutorNotified + Unpin>(_observer: Observer<T,E>) {
            fn assert_unpin<T: Unpin>() {}
            assert_unpin::<Observer<T,E>>();
        }
    }
}