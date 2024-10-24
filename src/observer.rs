use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

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



struct Shared<T> {
    //for now, we implement this with a mutex
    lock: std::sync::Mutex<Observation<T>>,
}


/**
Observes information about a task.
*/
pub struct Observer<T> {
    shared: Arc<Shared<T>>
}

pub(crate) struct ObserverSender<T,Notifier> {
    shared: Arc<Shared<T>>,
    notifier: Option<Notifier>
}

impl<T,Notifier> ObserverSender<T,Notifier> {
    pub(crate) fn send(&self, value: T) {
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

impl<T> Observer<T> {
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
}

pub trait ObserverNotifier<T> {
    fn notify(&mut self, value: &T);
}

pub struct NoNotifier;
impl<T> ObserverNotifier<T> for NoNotifier {
    fn notify(&mut self, _value: &T) {
        panic!("NoNotifier should not be used");
    }
}

pub(crate) fn observer_channel<R,Notifier>(notify: Option<Notifier>) -> (ObserverSender<R,Notifier>, Observer<R>) {
    let shared = Arc::new(Shared { lock: std::sync::Mutex::new(Observation::Pending) });
    (ObserverSender {shared: shared.clone(), notifier: notify}, Observer {shared})
}