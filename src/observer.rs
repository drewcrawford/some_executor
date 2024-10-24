use std::sync::Arc;

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
    pub(crate) notifier: Option<Notifier>
}

impl<T,Notifier> ObserverSender<T,Notifier> {
    pub(crate) fn send(&mut self, value: T) where Notifier: ObserverNotifier<T> {
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

/**
Provides inline notifications when a task completes.

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
pub trait ObserverNotifier<T>: Unpin {

    /**
    This function will be run inline when the task completes.
*/
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