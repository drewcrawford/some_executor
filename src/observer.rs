//SPDX-License-Identifier: MIT OR Apache-2.0

use crate::task::{InFlightTaskCancellation, TaskID};
use atomic_waker::AtomicWaker;
use std::any::Any;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::task::{Context, Poll};

/// Represents the state of an observed task.
///
/// This enum is returned by [`Observer::observe`] to indicate the current status
/// of a task being observed. Once a task completes with [`Observation::Ready`],
/// subsequent observations will return [`Observation::Done`].
///
/// # Examples
///
/// ```
/// use some_executor::observer::Observation;
///
/// // An observation can be constructed from a value
/// let obs: Observation<i32> = Observation::from(42);
/// assert_eq!(obs, Observation::Ready(42));
///
/// // Observations can be converted to Option
/// let ready: Observation<String> = Observation::Ready("hello".to_string());
/// let opt: Option<String> = ready.into();
/// assert_eq!(opt, Some("hello".to_string()));
///
/// let pending: Observation<String> = Observation::Pending;
/// let opt: Option<String> = pending.into();
/// assert_eq!(opt, None);
/// ```
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum Observation<T> {
    /// The task is still running and has not completed.
    Pending,
    /// The task has completed with a value.
    ///
    /// This value can only be observed once. After the value is taken,
    /// subsequent observations will return [`Observation::Done`].
    Ready(T),
    /// The task was previously completed and its value has already been observed.
    ///
    /// This state indicates that [`Observation::Ready`] was previously returned
    /// and the value has been consumed.
    Done,
    /// The task was cancelled before completion.
    ///
    /// Cancellation can occur when an [`Observer`] is dropped without calling
    /// [`Observer::detach`].
    Cancelled,
}

/// Represents the final state of an observed task.
///
/// This enum is returned when awaiting an [`Observer`] as a [`Future`].
/// Unlike [`Observation`], this only includes terminal states - either
/// the task completed successfully or was cancelled.
///
/// # Examples
///
/// ```
/// # use some_executor::observer::FinishedObservation;
/// # async fn example() {
/// # let observer: some_executor::observer::TypedObserver<String, std::convert::Infallible> = todo!();
/// // When awaiting an observer, you get a FinishedObservation
/// match observer.await {
///     FinishedObservation::Ready(value) => {
///         println!("Task completed with: {}", value);
///     }
///     FinishedObservation::Cancelled => {
///         println!("Task was cancelled");
///     }
/// }
/// # }
/// ```
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum FinishedObservation<T> {
    /// The task completed successfully with a value.
    Ready(T),
    /// The task was cancelled before completion.
    Cancelled,
}

#[derive(Debug)]
struct Shared<T> {
    //for now, we implement this with a mutex
    lock: std::sync::Mutex<Observation<T>>,
    /**
    To be notified when the task is cancelled.
    */
    waker: AtomicWaker,
    /**
    Indicates the observer was dropped without detach.
    */
    observer_cancelled: AtomicBool,

    in_flight_task_cancellation: InFlightTaskCancellation,
}

/// Provides observation and control over spawned tasks.
///
/// An `Observer` allows you to:
/// - Check the current state of a task without blocking using [`observe`](Self::observe)
/// - Wait for task completion by awaiting the observer (it implements [`Future`])
/// - Cancel a task by dropping the observer
/// - Detach from a task to let it run independently using [`detach`](Self::detach)
///
/// # Cancellation
///
/// Dropping an observer requests cancellation of the associated task. Cancellation in
/// some_executor is optimistic and operates at three levels:
///
/// 1. **Lightweight cancellation**: some_executor guarantees that polls occurring logically
///    after cancellation will not be run. This is free and universal.
/// 2. **In-flight cancellation**: If the task is currently being polled, cancellation depends
///    on how the task itself reacts to cancellation through the `IS_CANCELLED` task local.
///    Task support for this is sporadic and not guaranteed.
/// 3. **Executor cancellation**: The executor may support cancellation by dropping the future
///    and not running it again. This is not guaranteed and depends on the executor implementation.
///
/// To allow a task to continue running without the observer, use [`detach`](Self::detach).
///
/// # Examples
///
/// ```
/// # use some_executor::observer::{Observer, TypedObserver};
/// # use some_executor::observer::{Observation, FinishedObservation};
/// # async fn example() {
/// # let observer: TypedObserver<String, std::convert::Infallible> = todo!();
/// // Check task state without blocking
/// match observer.observe() {
///     Observation::Pending => println!("Task still running"),
///     Observation::Ready(value) => println!("Task completed: {}", value),
///     Observation::Done => println!("Value already taken"),
///     Observation::Cancelled => println!("Task was cancelled"),
/// }
///
/// // Or wait for completion
/// # let observer: TypedObserver<String, std::convert::Infallible> = todo!();
/// match observer.await {
///     FinishedObservation::Ready(value) => println!("Got: {}", value),
///     FinishedObservation::Cancelled => println!("Cancelled"),
/// }
///
/// // Detach to let task run independently
/// # let observer: TypedObserver<String, std::convert::Infallible> = todo!();
/// observer.detach(); // Task continues running
/// # }
/// ```
#[must_use]
pub trait Observer: 'static + Future<Output = FinishedObservation<Self::Value>> {
    /// The type of value produced by the observed task.
    type Value;

    /// Checks the current state of the task without blocking.
    ///
    /// This method allows you to inspect the task's progress without waiting.
    /// Note that [`Observation::Ready`] can only be returned once - subsequent
    /// calls will return [`Observation::Done`] after the value has been taken.
    fn observe(&self) -> Observation<Self::Value>;

    /// Returns the unique identifier of the observed task.
    fn task_id(&self) -> &TaskID;

    /// Detaches from the task, allowing it to continue running independently.
    ///
    /// After calling this method, the task will continue to execute even though
    /// the observer has been dropped. This prevents the cancellation that would
    /// normally occur when an observer is dropped.
    fn detach(self)
    where
        Self: Sized,
    {
        std::mem::forget(self);
    }
}

/// A concrete implementation of [`Observer`] for tasks that return a specific type.
///
/// `TypedObserver` is the primary way to observe tasks spawned on executors. It provides
/// both synchronous observation through [`observe`](Self::observe) and asynchronous
/// completion through its [`Future`] implementation.
///
/// The `ENotifier` type parameter allows executors to receive notifications about
/// observer lifecycle events (like cancellation requests). Most users can use
/// [`std::convert::Infallible`] for this parameter if executor notifications aren't needed.
///
/// # Examples
///
/// ```
/// # use some_executor::observer::TypedObserver;
/// # use std::convert::Infallible;
/// # async fn example() {
/// # let observer: TypedObserver<i32, Infallible> = todo!();
/// // Spawn a task and get an observer
/// // let observer = executor.spawn(task);
///
/// // Check status without blocking
/// use some_executor::observer::Observation;
/// if let Observation::Ready(value) = observer.observe() {
///     println!("Task completed with: {}", value);
/// }
///
/// // Or wait for completion
/// # let observer: TypedObserver<i32, Infallible> = todo!();
/// let result = observer.await;
/// println!("Final result: {:?}", result);
/// # }
/// ```
///
/// # Cancellation
///
/// Dropping a `TypedObserver` will request cancellation of the associated task:
///
/// ```no_run
/// # use some_executor::observer::TypedObserver;
/// # use std::convert::Infallible;
/// # let observer: TypedObserver<(), Infallible> = todo!();
/// // This will cancel the task
/// drop(observer);
///
/// // To avoid cancellation, detach the observer
/// # let observer: TypedObserver<(), Infallible> = todo!();
/// observer.detach(); // Task continues running
/// ```
#[derive(Debug)]
pub struct TypedObserver<T, ENotifier: ExecutorNotified> {
    shared: Arc<Shared<T>>,
    task_id: TaskID,
    notifier: Option<ENotifier>,
    detached: bool,
}

impl<T, ENotifier: ExecutorNotified> Drop for TypedObserver<T, ENotifier> {
    fn drop(&mut self) {
        if !self.detached {
            self.shared
                .observer_cancelled
                .store(true, std::sync::atomic::Ordering::Relaxed);
            self.shared.in_flight_task_cancellation.cancel();
            if let Some(mut n) = self.notifier.take() {
                n.request_cancel()
            }
        }
    }
}

impl<T: 'static, ENotifier: ExecutorNotified> Observer for TypedObserver<T, ENotifier> {
    type Value = T;

    fn observe(&self) -> Observation<Self::Value> {
        TypedObserver::observe(self)
    }

    fn task_id(&self) -> &TaskID {
        TypedObserver::task_id(self)
    }
}

/**
The sender side of an observer.  This side is held by the executor, and is used to send values to the observer.
*/
#[derive(Debug)]
pub(crate) struct ObserverSender<T, Notifier> {
    shared: Arc<Shared<T>>,
    pub(crate) notifier: Option<Notifier>,
}

impl<T, Notifier> ObserverSender<T, Notifier> {
    pub(crate) fn send(&mut self, value: T)
    where
        Notifier: ObserverNotified<T>,
    {
        if let Some(n) = self.notifier.as_mut() {
            n.notify(&value)
        }
        let mut lock = self.shared.lock.lock().unwrap();
        match *lock {
            Observation::Pending => {
                *lock = Observation::Ready(value);
                self.shared.waker.wake();
            }
            Observation::Ready(_) => {
                panic!("Observer already has a value");
            }
            Observation::Done => {
                panic!("Observer already completed");
            }
            Observation::Cancelled => {
                panic!("Observer cancelled");
            }
        }
    }

    pub(crate) fn observer_cancelled(&self) -> bool {
        self.shared
            .observer_cancelled
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl<T, Notifier> Drop for ObserverSender<T, Notifier> {
    fn drop(&mut self) {
        self.shared.in_flight_task_cancellation.cancel();
        let mut lock = self.shared.lock.lock().unwrap();
        match *lock {
            Observation::Pending => {
                *lock = Observation::Cancelled;
                self.shared.waker.wake();
            }
            Observation::Ready(_) => {
                //nothing to do
            }
            Observation::Done => {
                //nothing to do
            }
            Observation::Cancelled => {
                panic!("Observer cancelled");
            }
        }
    }
}

impl<T, E: ExecutorNotified> TypedObserver<T, E> {
    /// Checks the current state of the observed task.
    ///
    /// This method provides a non-blocking way to check if a task has completed.
    /// The first call after task completion will return [`Observation::Ready`] with
    /// the value, and subsequent calls will return [`Observation::Done`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use some_executor::observer::TypedObserver;
    /// # use some_executor::observer::Observation;
    /// # use std::convert::Infallible;
    /// # let observer: TypedObserver<String, Infallible> = todo!();
    /// match observer.observe() {
    ///     Observation::Pending => println!("Still working..."),
    ///     Observation::Ready(result) => println!("Got result: {}", result),
    ///     Observation::Done => println!("Already retrieved the result"),
    ///     Observation::Cancelled => println!("Task was cancelled"),
    /// }
    /// ```
    pub fn observe(&self) -> Observation<T> {
        let mut lock = self.shared.lock.lock().unwrap();
        match *lock {
            Observation::Pending => Observation::Pending,
            Observation::Ready(..) => std::mem::replace(&mut *lock, Observation::Done),
            Observation::Done => Observation::Done,
            Observation::Cancelled => Observation::Cancelled,
        }
    }

    /// Returns the unique identifier of the task being observed.
    ///
    /// Each spawned task has a unique [`TaskID`] that can be used for logging,
    /// debugging, or tracking purposes.
    pub fn task_id(&self) -> &TaskID {
        &self.task_id
    }

    /// Detaches from the task, allowing it to continue running independently.
    ///
    /// After calling this method, the task will continue to execute even after
    /// this observer is dropped. This is useful for "fire-and-forget" operations
    /// where you don't need the task's result and don't want to cancel it.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use some_executor::observer::TypedObserver;
    /// # use std::convert::Infallible;
    /// # let observer: TypedObserver<(), Infallible> = todo!();
    /// // Start a background task
    /// observer.detach();
    /// // The task continues running even though we no longer hold the observer
    /// ```
    pub fn detach(mut self) {
        self.notifier.take();
        self.detached = true;
    }
}

impl<T, E> Future for TypedObserver<T, E>
where
    E: ExecutorNotified,
{
    type Output = FinishedObservation<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.shared.waker.register(cx.waker());
        let o = self.observe();
        match o {
            Observation::Pending => Poll::Pending,
            Observation::Ready(v) => Poll::Ready(FinishedObservation::Ready(v)),
            Observation::Done => {
                unreachable!("Observer should not be polled after completion");
            }
            Observation::Cancelled => Poll::Ready(FinishedObservation::Cancelled),
        }
    }
}

/// Provides inline notifications when an observed task completes.
///
/// Unlike [`Observer`] which requires polling or awaiting, `ObserverNotified` allows
/// you to receive immediate notification when a task completes. The notifier's
/// [`notify`](Self::notify) method is called inline by the executor when the task
/// finishes.
///
/// # Cancellation Behavior
///
/// When a task is cancelled, the notifier is dropped without calling [`notify`](Self::notify).
/// This allows you to detect cancellation by implementing [`Drop`] for your notifier type.
///
/// # Design Note
///
/// This trait requires [`Unpin`] to allow the notifier to be moved during task execution.
/// This design choice enables notifiers to maintain mutable state without requiring
/// interior mutability patterns.
///
/// # Examples
///
/// ```
/// use some_executor::observer::ObserverNotified;
/// use std::sync::mpsc;
///
/// // A simple notifier that sends results through a channel
/// struct ChannelNotifier<T> {
///     sender: mpsc::Sender<T>,
/// }
///
/// impl<T: Clone + 'static> ObserverNotified<T> for ChannelNotifier<T> {
///     fn notify(&mut self, value: &T) {
///         let _ = self.sender.send(value.clone());
///     }
/// }
///
/// // A notifier that logs completion
/// struct LogNotifier {
///     task_name: String,
/// }
///
/// impl<T: std::fmt::Debug + ?Sized> ObserverNotified<T> for LogNotifier {
///     fn notify(&mut self, value: &T) {
///         println!("Task '{}' completed with: {:?}", self.task_name, value);
///     }
/// }
///
/// impl Drop for LogNotifier {
///     fn drop(&mut self) {
///         println!("Task '{}' was cancelled or notifier dropped", self.task_name);
///     }
/// }
/// ```
pub trait ObserverNotified<T: ?Sized>: Unpin + 'static {
    /// Called inline when the observed task completes successfully.
    ///
    /// This method is invoked by the executor immediately when the task finishes.
    /// It will not be called if the task is cancelled.
    ///
    /// # Parameters
    /// - `value`: A reference to the task's output value
    fn notify(&mut self, value: &T);
}

/// Allows executors to receive notifications about observer lifecycle events.
///
/// This trait enables executors to be notified when observers request task cancellation.
/// While implementing cancellation support is optional, it can improve efficiency by
/// allowing executors to stop polling cancelled tasks.
///
/// # Using Without Notifications
///
/// If your executor doesn't need notifications, use [`std::convert::Infallible`] as
/// the type parameter and pass `None` where executor notifiers are expected:
///
/// ```ignore
/// use std::convert::Infallible;
/// use some_executor::SomeExecutor;
///
/// // Define an executor that doesn't use notifications
/// struct MyExecutor;
///
/// impl SomeExecutor for MyExecutor {
///     type ExecutorNotifier = Infallible;
///     
///     // Implement the required methods...
///     // (implementation details omitted for brevity)
/// }
/// ```
///
/// # Examples
///
/// ```
/// use some_executor::observer::ExecutorNotified;
///
/// // A simple notifier that tracks cancellation requests
/// struct CancellationTracker {
///     task_id: u64,
///     cancelled: bool,
/// }
///
/// impl ExecutorNotified for CancellationTracker {
///     fn request_cancel(&mut self) {
///         self.cancelled = true;
///         println!("Task {} cancellation requested", self.task_id);
///         // Executor can now stop polling this task
///     }
/// }
/// ```
pub trait ExecutorNotified: 'static {
    /// Called when an observer requests cancellation of its associated task.
    ///
    /// Executors can use this notification to stop polling the task, though
    /// implementing this is optional. The cancellation is already tracked through
    /// other mechanisms, so this is purely an optimization opportunity.
    fn request_cancel(&mut self);
}

impl<T> ObserverNotified<T> for Infallible {
    fn notify(&mut self, _value: &T) {
        panic!("NoNotified should not be used");
    }
}
//support unboxing
impl ObserverNotified<Box<(dyn std::any::Any + 'static)>>
    for Box<dyn ObserverNotified<(dyn std::any::Any + 'static)>>
{
    fn notify(&mut self, value: &Box<(dyn Any + 'static)>) {
        let r = Box::as_mut(self);
        r.notify(value);
    }
}

//I guess a few ways?

impl ObserverNotified<Box<(dyn Any + Send + 'static)>>
    for Box<dyn ObserverNotified<(dyn Any + Send + 'static)> + Send>
{
    fn notify(&mut self, value: &Box<(dyn Any + Send + 'static)>) {
        let r = Box::as_mut(self);
        r.notify(value);
    }
}

impl ExecutorNotified for Infallible {
    fn request_cancel(&mut self) {
        panic!("NoNotified should not be used");
    }
}

pub(crate) fn observer_channel<R, ONotifier, ENotifier: ExecutorNotified>(
    observer_notify: Option<ONotifier>,
    executor_notify: Option<ENotifier>,
    task_cancellation: InFlightTaskCancellation,
    task_id: TaskID,
) -> (ObserverSender<R, ONotifier>, TypedObserver<R, ENotifier>) {
    let shared = Arc::new(Shared {
        lock: std::sync::Mutex::new(Observation::Pending),
        waker: AtomicWaker::new(),
        observer_cancelled: AtomicBool::new(false),
        in_flight_task_cancellation: task_cancellation,
    });
    (
        ObserverSender {
            shared: shared.clone(),
            notifier: observer_notify,
        },
        TypedObserver {
            shared,
            task_id,
            notifier: executor_notify,
            detached: false,
        },
    )
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
impl ExecutorNotified for Box<dyn ExecutorNotified + Send> {
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
impl<T> From<T> for Observation<T> {
    fn from(value: T) -> Self {
        Observation::Ready(value)
    }
}

impl<T> From<Observation<T>> for Option<T> {
    fn from(value: Observation<T>) -> Self {
        match value {
            Observation::Ready(v) => Some(v),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::observer::{ExecutorNotified, TypedObserver};

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_send() {
        /* observer can send when the underlying value can */
        #[allow(unused)]
        fn ex<'executor, T: Send, E: ExecutorNotified + Send>(_observer: TypedObserver<T, E>) {
            fn assert_send<T: Send>() {}
            assert_send::<TypedObserver<T, E>>();
        }
    }
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn test_unpin() {
        /* observer can unpin */
        #[allow(unused)]
        fn ex<'executor, T, E: ExecutorNotified + Unpin>(_observer: TypedObserver<T, E>) {
            fn assert_unpin<T: Unpin>() {}
            assert_unpin::<TypedObserver<T, E>>();
        }
    }
}
