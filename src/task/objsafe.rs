// SPDX-License-Identifier: MIT OR Apache-2.0

//! Object-safe task creation methods.
//!
//! This module contains specialized constructors for creating tasks that can be used
//! with object-safe trait methods. These tasks use type erasure to enable dynamic dispatch
//! while maintaining performance for the executor side.

use super::{Configuration, Task};
use crate::observer::ObserverNotified;
use std::any::Any;
use std::future::Future;
use std::pin::Pin;

// Type aliases for complex types to reduce duplication and improve readability

/// Type alias for a boxed future that outputs boxed Any and is Send + 'static
pub type BoxedSendFuture =
    Pin<Box<dyn Future<Output = Box<dyn Any + 'static + Send>> + 'static + Send>>;

/// Type alias for a boxed observer notifier that handles Send Any values
pub type BoxedSendObserverNotifier = Box<dyn ObserverNotified<dyn Any + Send> + Send>;

/// Type alias for a Task that can be used with object-safe spawning
pub type ObjSafeTask = Task<BoxedSendFuture, BoxedSendObserverNotifier>;

/// Type alias for a boxed future that outputs boxed Any (non-Send)
pub type BoxedLocalFuture = Pin<Box<dyn Future<Output = Box<dyn Any + 'static>> + 'static>>;

/// Type alias for a boxed observer notifier that handles Any values (non-Send)
pub type BoxedLocalObserverNotifier = Box<dyn ObserverNotified<dyn Any + 'static>>;

/// Type alias for a Task that can be used with local object-safe spawning
pub type ObjSafeLocalTask = Task<BoxedLocalFuture, BoxedLocalObserverNotifier>;

/// Type alias for a Task that can be used with static object-safe spawning
pub type ObjSafeStaticTask = Task<BoxedLocalFuture, BoxedLocalObserverNotifier>;

impl Task<BoxedSendFuture, BoxedSendObserverNotifier> {
    /// Creates a new object-safe task for type-erased spawning.
    ///
    /// This constructor is used internally to create tasks that can be spawned
    /// using object-safe trait methods, where the concrete future type is erased.
    ///
    /// # Arguments
    ///
    /// * `label` - A human-readable label for the task
    /// * `future` - A boxed future that outputs a boxed `Any + Send` value
    /// * `configuration` - Runtime hints and scheduling preferences
    /// * `notifier` - Optional boxed observer for completion notifications
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration};
    /// use std::any::Any;
    ///
    /// let task = Task::new_objsafe(
    ///     "type-erased".to_string(),
    ///     Box::new(async {
    ///         Box::new(42) as Box<dyn Any + Send>
    ///     }),
    ///     Configuration::default(),
    ///     None,
    /// );
    /// ```
    pub fn new_objsafe(
        label: String,
        future: Box<dyn Future<Output = Box<dyn Any + Send + 'static>> + Send + 'static>,
        configuration: Configuration,
        notifier: Option<Box<dyn ObserverNotified<dyn Any + Send> + Send>>,
    ) -> Self {
        Self::with_notifications(label, configuration, notifier, Box::into_pin(future))
    }
}

impl Task<BoxedLocalFuture, BoxedLocalObserverNotifier> {
    /// Creates a new object-safe task for local (non-Send) type-erased spawning.
    ///
    /// This constructor is used internally to create tasks that can be spawned
    /// on local executors using object-safe trait methods. The task doesn't need
    /// to be `Send` but must be `'static`.
    ///
    /// # Arguments
    ///
    /// * `label` - A human-readable label for the task
    /// * `future` - A boxed future that outputs a boxed `Any` value
    /// * `configuration` - Runtime hints and scheduling preferences
    /// * `notifier` - Optional boxed observer for completion notifications
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::{Task, Configuration};
    /// use std::any::Any;
    /// use std::rc::Rc;
    ///
    /// // Rc is !Send
    /// let data = Rc::new(42);
    ///
    /// let task = Task::new_objsafe_local(
    ///     "local-type-erased".to_string(),
    ///     Box::new(async move {
    ///         Box::new(*data) as Box<dyn Any>
    ///     }),
    ///     Configuration::default(),
    ///     None,
    /// );
    /// ```
    pub fn new_objsafe_local(
        label: String,
        future: Box<dyn Future<Output = Box<dyn Any + 'static>> + 'static>,
        configuration: Configuration,
        notifier: Option<Box<dyn ObserverNotified<dyn Any + 'static>>>,
    ) -> Self {
        Self::with_notifications(label, configuration, notifier, Box::into_pin(future))
    }

    /// Creates a new task suitable for static objsafe spawning.
    ///
    /// This constructor is used internally by [`into_objsafe_static`](Task::into_objsafe_static)
    /// to create tasks that can be spawned on static executors using object-safe methods.
    pub fn new_objsafe_static(
        label: String,
        future: Box<dyn Future<Output = Box<dyn Any + 'static>> + 'static>,
        configuration: Configuration,
        notifier: Option<Box<dyn ObserverNotified<dyn Any + 'static>>>,
    ) -> Self {
        Self::with_notifications(label, configuration, notifier, Box::into_pin(future))
    }
}
