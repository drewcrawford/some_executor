// SPDX-License-Identifier: MIT OR Apache-2.0

//! Task configuration types for the some_executor framework.
//!
//! This module provides configuration types that allow fine-grained control over task execution
//! behavior. Configurations convey scheduling hints, priorities, and timing constraints to
//! executors, enabling them to make optimal scheduling decisions based on task characteristics.
//!
//! # Overview
//!
//! The configuration system consists of two main types:
//!
//! - [`Configuration`]: The immutable configuration data that accompanies a task
//! - [`ConfigurationBuilder`]: A builder pattern for constructing configurations fluently
//!
//! Configurations influence how executors schedule and prioritize tasks but do not guarantee
//! specific behavior - executors may interpret these hints based on their implementation.
//!
//! # Examples
//!
//! ## Basic Configuration
//!
//! ```
//! use some_executor::task::{Configuration, ConfigurationBuilder};
//! use some_executor::hint::Hint;
//! use some_executor::Priority;
//! use some_executor::Instant;
//!
//! // Direct construction when all values are known
//! let config = Configuration::new(
//!     Hint::CPU,
//!     Priority::unit_test(),
//!     Instant::now()
//! );
//!
//! // Builder pattern for flexible construction
//! let config = ConfigurationBuilder::new()
//!     .hint(Hint::IO)
//!     .priority(Priority::unit_test())
//!     .build();
//! ```
//!
//! ## Delayed Task Execution
//!
//! ```
//! use some_executor::task::ConfigurationBuilder;
//! use some_executor::Instant;
//! use std::time::Duration;
//!
//! // Schedule a task to run after a delay
//! let delayed_config = ConfigurationBuilder::new()
//!     .poll_after(Instant::now() + Duration::from_secs(5))
//!     .build();
//! ```

use crate::Priority;
use crate::hint::Hint;

/// Configuration for task execution.
///
/// `Configuration` encapsulates metadata that guides executor scheduling decisions.
/// Each configuration consists of:
///
/// - **Hint**: Indicates the expected behavior of the task (CPU-bound, I/O-bound, etc.)
/// - **Priority**: Determines scheduling precedence when multiple tasks are ready
/// - **Poll After**: Specifies the earliest time a task should be polled
///
/// Configurations are immutable once created and can be shared across multiple tasks.
/// Executors use this information as hints for optimization but are not required to
/// strictly follow them.
///
/// # Examples
///
/// ## Creating a configuration for a CPU-intensive task
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
///
/// assert_eq!(config.hint(), Hint::CPU);
/// assert_eq!(config.priority(), Priority::unit_test());
/// ```
///
/// ## Using default configuration
///
/// ```
/// use some_executor::task::Configuration;
/// use some_executor::hint::Hint;
/// use some_executor::Priority;
///
/// let config = Configuration::default();
///
/// // Default values are sensible for general use
/// assert_eq!(config.hint(), Hint::Unknown);
/// assert_eq!(config.priority(), Priority::Unknown);
/// ```
///
/// ## Comparing configurations
///
/// ```
/// use some_executor::task::Configuration;
/// use some_executor::hint::Hint;
/// use some_executor::Priority;
/// use some_executor::Instant;
///
/// let config1 = Configuration::new(
///     Hint::IO,
///     Priority::unit_test(),
///     Instant::now()
/// );
///
/// let config2 = config1.clone();
/// assert_eq!(config1, config2);
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
/// with optional settings. This pattern allows for flexible configuration creation
/// where only the relevant parameters need to be specified.
///
/// # Default Values
///
/// When built, any unspecified values will use these defaults:
/// - `hint`: [`Hint::Unknown`]
/// - `priority`: [`Priority::Unknown`]
/// - `poll_after`: The current instant (via `Instant::now()`)
///
/// # Examples
///
/// ## Building with all parameters
///
/// ```
/// use some_executor::task::ConfigurationBuilder;
/// use some_executor::hint::Hint;
/// use some_executor::Priority;
/// use some_executor::Instant;
/// use std::time::Duration;
///
/// let config = ConfigurationBuilder::new()
///     .hint(Hint::CPU)
///     .priority(Priority::unit_test())
///     .poll_after(Instant::now() + Duration::from_secs(2))
///     .build();
///
/// assert_eq!(config.hint(), Hint::CPU);
/// assert_eq!(config.priority(), Priority::unit_test());
/// ```
///
/// ## Building with partial parameters
///
/// ```
/// use some_executor::task::ConfigurationBuilder;
/// use some_executor::hint::Hint;
/// use some_executor::Priority;
///
/// // Only set what's needed; others use defaults
/// let config = ConfigurationBuilder::new()
///     .hint(Hint::IO)
///     .build();
///
/// assert_eq!(config.hint(), Hint::IO);
/// assert_eq!(config.priority(), Priority::Unknown); // Default
/// ```
///
/// ## Using the Default trait
///
/// ```
/// use some_executor::task::ConfigurationBuilder;
/// use some_executor::hint::Hint;
///
/// // ConfigurationBuilder implements Default
/// let config = ConfigurationBuilder::default()
///     .hint(Hint::CPU)
///     .build();
///
/// assert_eq!(config.hint(), Hint::CPU);
/// ```
///
/// ## Chaining multiple builder calls
///
/// ```
/// use some_executor::task::ConfigurationBuilder;
/// use some_executor::hint::Hint;
/// use some_executor::Priority;
/// use some_executor::Instant;
/// use std::time::Duration;
///
/// let base_time = Instant::now();
/// let config = ConfigurationBuilder::new()
///     .hint(Hint::IO)
///     .priority(Priority::unit_test())
///     .poll_after(base_time + Duration::from_millis(100))
///     .build();
///
/// // All values are set as expected
/// assert_eq!(config.hint(), Hint::IO);
/// assert_eq!(config.priority(), Priority::unit_test());
/// assert!(config.poll_after() >= base_time);
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
    /// This is equivalent to calling [`ConfigurationBuilder::default()`].
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::ConfigurationBuilder;
    ///
    /// let builder = ConfigurationBuilder::new();
    /// let config = builder.build();
    ///
    /// // All fields will have default values
    /// use some_executor::hint::Hint;
    /// use some_executor::Priority;
    /// assert_eq!(config.hint(), Hint::Unknown);
    /// assert_eq!(config.priority(), Priority::Unknown);
    /// ```
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
    /// Consumes the builder and produces an immutable `Configuration`.
    /// Any unset values will use their defaults:
    /// - `hint`: [`Hint::Unknown`] (via `Hint::default()`)
    /// - `priority`: [`Priority::Unknown`]
    /// - `poll_after`: The current instant (via `Instant::now()`)
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::ConfigurationBuilder;
    /// use some_executor::hint::Hint;
    /// use some_executor::Priority;
    ///
    /// // Building with no values set uses all defaults
    /// let config = ConfigurationBuilder::new().build();
    /// assert_eq!(config.hint(), Hint::Unknown);
    /// assert_eq!(config.priority(), Priority::Unknown);
    ///
    /// // Building with some values set
    /// let config = ConfigurationBuilder::new()
    ///     .hint(Hint::CPU)
    ///     .build();
    /// assert_eq!(config.hint(), Hint::CPU);
    /// assert_eq!(config.priority(), Priority::Unknown); // Still default
    /// ```
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
    ///
    /// The hint indicates the expected runtime characteristics of the task,
    /// such as whether it's CPU-bound or I/O-bound.
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
    ///     Hint::IO,
    ///     Priority::unit_test(),
    ///     Instant::now()
    /// );
    ///
    /// assert_eq!(config.hint(), Hint::IO);
    /// ```
    pub fn hint(&self) -> Hint {
        self.hint
    }

    /// Returns the priority for this configuration.
    ///
    /// The priority influences task scheduling order when multiple tasks
    /// are ready to execute.
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
    ///
    /// assert_eq!(config.priority(), Priority::unit_test());
    /// ```
    pub fn priority(&self) -> priority::Priority {
        self.priority
    }

    /// Returns the earliest time the task should be polled.
    ///
    /// Tasks will not be scheduled for execution before this instant,
    /// allowing for delayed or scheduled task execution.
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::Configuration;
    /// use some_executor::hint::Hint;
    /// use some_executor::Priority;
    /// use some_executor::Instant;
    /// use std::time::Duration;
    ///
    /// let future_time = Instant::now() + Duration::from_secs(10);
    /// let config = Configuration::new(
    ///     Hint::Unknown,
    ///     Priority::Unknown,
    ///     future_time
    /// );
    ///
    /// assert!(config.poll_after() >= Instant::now());
    /// ```
    pub fn poll_after(&self) -> crate::sys::Instant {
        self.poll_after
    }
}

impl Default for Configuration {
    /// Creates a default configuration with sensible defaults.
    ///
    /// The default configuration uses:
    /// - `hint`: [`Hint::Unknown`]
    /// - `priority`: [`Priority::Unknown`]
    /// - `poll_after`: The current instant
    ///
    /// # Examples
    ///
    /// ```
    /// use some_executor::task::Configuration;
    /// use some_executor::hint::Hint;
    /// use some_executor::Priority;
    /// use some_executor::Instant;
    ///
    /// let config = Configuration::default();
    ///
    /// assert_eq!(config.hint(), Hint::Unknown);
    /// assert_eq!(config.priority(), Priority::Unknown);
    /// // poll_after will be approximately now
    /// assert!(config.poll_after() <= Instant::now() + std::time::Duration::from_millis(100));
    /// ```
    fn default() -> Self {
        Configuration {
            hint: Hint::default(),
            priority: priority::Priority::Unknown,
            poll_after: crate::sys::Instant::now(),
        }
    }
}
