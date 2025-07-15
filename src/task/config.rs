// SPDX-License-Identifier: MIT OR Apache-2.0

//! Task configuration types for the some_executor framework.
//!
//! This module provides configuration types for tasks, including hints, priorities,
//! and scheduling preferences that executors can use to make optimal scheduling decisions.

use crate::Priority;
use crate::hint::Hint;

/// Configuration for task execution.
///
/// `Configuration` holds metadata that provides hints to executors about how a task
/// should be scheduled and executed. This includes execution hints, priority levels,
/// and timing constraints.
///
/// # Examples
///
/// ```
/// use some_executor::task::Configuration;
/// use some_executor::hint::Hint;
/// use some_executor::Priority;
/// use some_executor::Instant;
///
/// let config = Configuration::new(
///     Hint::CPU,
///     Priority::unit_test(),
///     Instant::now()
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Configuration {
    hint: Hint,
    priority: priority::Priority,
    poll_after: crate::sys::Instant,
}

/// A builder for creating [`Configuration`] instances.
///
/// `ConfigurationBuilder` provides a fluent API for constructing task configurations
/// with optional settings. Any unspecified values will use sensible defaults.
///
/// # Examples
///
/// ```
/// use some_executor::task::ConfigurationBuilder;
/// use some_executor::hint::Hint;
/// use some_executor::Priority;
/// use some_executor::Instant;
/// use std::time::Duration;
///
/// // Build a configuration with specific settings
/// let config = ConfigurationBuilder::new()
///     .hint(Hint::CPU)
///     .priority(Priority::unit_test())
///     .poll_after(Instant::now() + Duration::from_secs(2))
///     .build();
///
/// // Use default builder - all fields will have default values
/// let default_config = ConfigurationBuilder::default().build();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ConfigurationBuilder {
    hint: Option<Hint>,
    priority: Option<Priority>,
    poll_after: Option<crate::sys::Instant>,
}

impl ConfigurationBuilder {
    /// Creates a new `ConfigurationBuilder` with no values set.
    ///
    /// All fields will be `None` until explicitly set via the builder methods.
    pub fn new() -> Self {
        ConfigurationBuilder {
            hint: None,
            priority: None,
            poll_after: None,
        }
    }

    /// Sets the execution hint for the task.
    ///
    /// Hints provide guidance to executors about the expected behavior of the task,
    /// allowing for more efficient scheduling decisions.
    ///
    /// # Arguments
    ///
    /// * `hint` - The execution hint for the task
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::ConfigurationBuilder;
    /// use some_executor::hint::Hint;
    ///
    /// let config = ConfigurationBuilder::new()
    ///     .hint(Hint::IO)
    ///     .build();
    /// ```
    pub fn hint(mut self, hint: Hint) -> Self {
        self.hint = Some(hint);
        self
    }

    /// Sets the priority for the task.
    ///
    /// Priority influences scheduling when multiple tasks are ready to run.
    /// Higher priority tasks are generally scheduled before lower priority ones.
    ///
    /// # Arguments
    ///
    /// * `priority` - The priority level for the task
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::ConfigurationBuilder;
    /// use some_executor::Priority;
    ///
    /// let config = ConfigurationBuilder::new()
    ///     .priority(Priority::unit_test())
    ///     .build();
    /// ```
    pub fn priority(mut self, priority: priority::Priority) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Sets the earliest time the task should be polled.
    ///
    /// This can be used to delay task execution or implement scheduled tasks.
    /// Executors will not poll the task before this time.
    ///
    /// # Arguments
    ///
    /// * `poll_after` - The instant after which the task can be polled
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::ConfigurationBuilder;
    /// use some_executor::Instant;
    /// use std::time::Duration;
    ///
    /// // Delay task execution by 5 seconds
    /// let config = ConfigurationBuilder::new()
    ///     .poll_after(Instant::now() + Duration::from_secs(5))
    ///     .build();
    /// ```
    pub fn poll_after(mut self, poll_after: crate::sys::Instant) -> Self {
        self.poll_after = Some(poll_after);
        self
    }

    /// Builds the final [`Configuration`] instance.
    ///
    /// Any unset values will use their defaults:
    /// - `hint`: `Hint::default()`
    /// - `priority`: `Priority::Unknown`
    /// - `poll_after`: `Instant::now()`
    pub fn build(self) -> Configuration {
        Configuration {
            hint: self.hint.unwrap_or_default(),
            priority: self.priority.unwrap_or(priority::Priority::Unknown),
            poll_after: self.poll_after.unwrap_or_else(crate::sys::Instant::now),
        }
    }
}

impl Configuration {
    /// Creates a new `Configuration` with the specified values.
    ///
    /// This is an alternative to using [`ConfigurationBuilder`] when you have all
    /// values available upfront.
    ///
    /// # Arguments
    ///
    /// * `hint` - The execution hint for the task
    /// * `priority` - The priority level for the task
    /// * `poll_after` - The earliest time the task should be polled
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::Configuration;
    /// use some_executor::hint::Hint;
    /// use some_executor::Priority;
    /// use some_executor::Instant;
    ///
    /// let config = Configuration::new(
    ///     Hint::CPU,
    ///     Priority::unit_test(),
    ///     Instant::now()
    /// );
    /// ```
    pub fn new(hint: Hint, priority: priority::Priority, poll_after: crate::sys::Instant) -> Self {
        Configuration {
            hint,
            priority,
            poll_after,
        }
    }

    /// Returns the execution hint for this configuration.
    pub fn hint(&self) -> Hint {
        self.hint
    }

    /// Returns the priority for this configuration.
    pub fn priority(&self) -> priority::Priority {
        self.priority
    }

    /// Returns the earliest time the task should be polled.
    pub fn poll_after(&self) -> crate::sys::Instant {
        self.poll_after
    }
}

impl Default for Configuration {
    fn default() -> Self {
        Configuration {
            hint: Hint::default(),
            priority: priority::Priority::Unknown,
            poll_after: crate::sys::Instant::now(),
        }
    }
}
