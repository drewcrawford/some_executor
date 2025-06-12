// SPDX-License-Identifier: MIT OR Apache-2.0

/*!
This executor is in use when no other executors are registered.

It is intentionally the simplest idea possible, but it ensures a compliant executor is always available.

> Cut my tasks into pieces, this is my last resort!
> Async handling, no tokio, don't give a fuck if performance is bleeding
> this is my last resort

*/

use crate::observer::{FinishedObservation, Observer, ObserverNotified};
use crate::task::Task;
use crate::{DynExecutor, SomeExecutor};
use std::any::Any;
use std::convert::Infallible;
use std::future::Future;
use std::pin::{Pin, pin};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, RawWaker, RawWakerVTable};

#[cfg(not(target_arch = "wasm32"))]
use std::thread;

#[cfg(target_arch = "wasm32")]
use wasm_thread as thread;

pub(crate) struct LastResortExecutor;

impl LastResortExecutor {
    pub fn new() -> Self {
        LastResortExecutor
    }
}

const SLEEPING: u8 = 0;
const LISTENING: u8 = 1;
const WAKEPLS: u8 = 2;

struct Shared {
    condvar: Condvar,
    mutex: Mutex<bool>,
    //have to be careful about deadlocks here,
    inline_notify: AtomicU8,
}
struct Waker {
    shared: Arc<Shared>,
}

const WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    |data| {
        let waker = unsafe { Arc::from_raw(data as *const Waker) };
        let w2 = waker.clone();
        std::mem::forget(waker);
        RawWaker::new(Arc::into_raw(w2) as *const (), &WAKER_VTABLE)
    },
    |data| {
        let waker = unsafe { Arc::from_raw(data as *const Waker) };
        let old = waker.shared.inline_notify.swap(WAKEPLS, Ordering::Relaxed);
        if old == SLEEPING {
            waker.shared.condvar.notify_one();
        }
        drop(waker);
    },
    |data| {
        let waker = unsafe { Arc::from_raw(data as *const Waker) };
        let old = waker.shared.inline_notify.swap(WAKEPLS, Ordering::Relaxed);
        if old == SLEEPING {
            waker.shared.condvar.notify_one();
        }
        std::mem::forget(waker);
    },
    |data| {
        let waker = unsafe { Arc::from_raw(data as *const Waker) };
        drop(waker)
    },
);
impl Waker {
    fn into_core_waker(self) -> core::task::Waker {
        let data = Arc::into_raw(Arc::new(self));
        unsafe { core::task::Waker::from_raw(RawWaker::new(data as *const (), &WAKER_VTABLE)) }
    }
}

impl LastResortExecutor {
    fn spawn<F: Future + Send + 'static>(f: F) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            eprintln!(
                "some_executor::LastResortExecutor is in use.  This is not intended for production code; investigate ways to use a production-quality executor."
            );
        }
        #[cfg(target_arch = "wasm32")]
        {
            web_sys::console::log_1(&"some_executor::LastResortExecutor is in use.  This is not intended for production code; investigate ways to use a production-quality executor.".into());
        }

        thread::spawn(|| {
            let shared = Arc::new(Shared {
                condvar: Condvar::new(),
                mutex: Mutex::new(false),
                inline_notify: AtomicU8::new(SLEEPING),
            });
            let waker = Waker {
                shared: shared.clone(),
            }
            .into_core_waker();
            let mut c = Context::from_waker(&waker);
            let mut pin = pin!(f);
            loop {
                let mut _guard = shared.mutex.lock().expect("Mutex poisoned");
                //eagerly poll
                shared.inline_notify.store(LISTENING, Ordering::Relaxed);
                let r = pin.as_mut().poll(&mut c);
                match r {
                    Poll::Ready(..) => {
                        return;
                    }
                    Poll::Pending => {
                        let old = shared.inline_notify.swap(SLEEPING, Ordering::Relaxed);
                        if old == WAKEPLS {
                            //release lock anyway
                            drop(_guard);
                            continue; //poll eagerly
                        } else {
                            _guard = shared
                                .condvar
                                .wait_while(_guard, |_| {
                                    shared.inline_notify.load(Ordering::Relaxed) != WAKEPLS
                                })
                                .expect("Condvar poisoned");
                        }
                    }
                }
            }
        });
    }
}

impl SomeExecutor for LastResortExecutor {
    type ExecutorNotifier = Infallible;

    fn spawn<F: Future + Send + 'static, Notifier: ObserverNotified<F::Output> + Send>(
        &mut self,
        task: Task<F, Notifier>,
    ) -> impl Observer<Value = F::Output>
    where
        Self: Sized,
        F::Output: Send + Unpin,
    {
        let (s, o) = task.spawn(self);
        Self::spawn(s);
        o
    }

    async fn spawn_async<F: Future + Send + 'static, Notifier: ObserverNotified<F::Output> + Send>(
        &mut self,
        task: Task<F, Notifier>,
    ) -> impl Observer<Value = F::Output>
    where
        Self: Sized,
        F::Output: Send + Unpin,
    {
        let (s, o) = task.spawn(self);
        Self::spawn(s);
        o
    }

    fn spawn_objsafe(
        &mut self,
        task: Task<
            Pin<Box<dyn Future<Output = Box<dyn Any + 'static + Send>> + 'static + Send>>,
            Box<dyn ObserverNotified<dyn Any + Send> + Send>,
        >,
    ) -> Box<
        dyn Observer<Value = Box<dyn Any + Send>, Output = FinishedObservation<Box<dyn Any + Send>>>
            + Send,
    > {
        let (s, o) = task.spawn_objsafe(self);
        Self::spawn(s);
        Box::new(o)
    }

    fn spawn_objsafe_async<'s>(
        &'s mut self,
        task: Task<
            Pin<Box<dyn Future<Output = Box<dyn Any + 'static + Send>> + 'static + Send>>,
            Box<dyn ObserverNotified<dyn Any + Send> + Send>,
        >,
    ) -> Box<
        dyn Future<
                Output = Box<
                    dyn Observer<
                            Value = Box<dyn Any + Send>,
                            Output = FinishedObservation<Box<dyn Any + Send>>,
                        > + Send,
                >,
            > + 's,
    > {
        #[allow(clippy::async_yields_async)]
        Box::new(async {
            let (s, o) = task.spawn_objsafe(self);
            Self::spawn(s);
            Box::new(o)
                as Box<
                    dyn Observer<
                            Value = Box<dyn Any + Send>,
                            Output = FinishedObservation<Box<dyn Any + Send>>,
                        > + Send,
                >
        })
    }

    fn clone_box(&self) -> Box<DynExecutor> {
        Box::new(Self)
    }

    fn executor_notifier(&mut self) -> Option<Self::ExecutorNotifier> {
        None
    }
}
