// SPDX-License-Identifier: MIT OR Apache-2.0

//! Standard library implementation for non-WASM32 targets
//!
//! This module contains the blocking synchronization primitives using std::sync::Condvar
//! and std::sync::Mutex, which work on all platforms except WASM32.

use crate::observer::ObserverNotified;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, RawWaker, RawWakerVTable};

// State constants for the inline notification system
pub(crate) const SLEEPING: u8 = 0;
pub(crate) const LISTENING: u8 = 1;
pub(crate) const WAKEPLS: u8 = 2;

pub(crate) struct Shared {
    pub(crate) condvar: Condvar,
    pub(crate) mutex: Mutex<bool>,
    pub(crate) inline_notify: AtomicU8,
}

pub(crate) struct Waker {
    pub(crate) shared: Arc<Shared>,
}

pub(crate) static WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
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
        drop(waker);
    },
);

impl Waker {
    pub(crate) fn into_core_waker(self) -> core::task::Waker {
        let data = Arc::into_raw(Arc::new(self));
        unsafe { core::task::Waker::from_raw(RawWaker::new(data as *const (), &WAKER_VTABLE)) }
    }
}

/// Helper to run a SpawnedLocalTask to completion using condvar/mutex
pub(crate) fn run_local_task<F, N>(
    mut spawned: crate::task::SpawnedLocalTask<
        F,
        N,
        crate::local_last_resort::LocalLastResortExecutor,
    >,
) where
    F: Future,
    N: ObserverNotified<F::Output>,
{
    let shared = Arc::new(Shared {
        condvar: Condvar::new(),
        mutex: Mutex::new(false),
        inline_notify: AtomicU8::new(SLEEPING),
    });
    let waker = Waker {
        shared: shared.clone(),
    }
    .into_core_waker();
    let mut context = Context::from_waker(&waker);
    let mut executor = crate::local_last_resort::LocalLastResortExecutor::new();
    let mut pinned = unsafe { Pin::new_unchecked(&mut spawned) };

    loop {
        let mut _guard = shared.mutex.lock().expect("Mutex poisoned");
        // Eagerly poll
        shared.inline_notify.store(LISTENING, Ordering::Relaxed);
        let r = pinned.as_mut().poll(&mut context, &mut executor, None);
        match r {
            Poll::Ready(()) => {
                return;
            }
            Poll::Pending => {
                let old = shared.inline_notify.swap(SLEEPING, Ordering::Relaxed);
                if old == WAKEPLS {
                    // Release lock anyway
                    drop(_guard);
                    continue; // Poll eagerly
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
}
