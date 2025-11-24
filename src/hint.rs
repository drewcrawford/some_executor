// SPDX-License-Identifier: MIT OR Apache-2.0

//! Execution hints for guiding executor scheduling decisions.
//!
//! This module provides the [`Hint`] type which allows tasks to communicate
//! their expected runtime characteristics to executors. These hints enable
//! executors to make more informed scheduling decisions, potentially improving
//! overall system performance.
//!
//! # Overview
//!
//! Different types of async tasks have different runtime profiles:
//!
//! - **I/O-bound tasks** spend most of their time waiting for external events
//!   (network, disk, timers) and yield frequently
//! - **CPU-bound tasks** spend most of their time computing and may run for
//!   extended periods without yielding
//!
//! Executors can use these hints to optimize scheduling. For example, an
//! executor might:
//! - Run CPU-bound tasks on dedicated worker threads
//! - Use work-stealing for I/O-bound tasks
//! - Apply different preemption policies based on the hint
//!
//! # Important Notes
//!
//! - Hints are advisory only; executors may ignore them
//! - Providing incorrect hints won't cause correctness issues but may impact performance
//! - When in doubt, use [`Hint::Unknown`]
//!
//! # Examples
//!
//! ## Marking a CPU-intensive task
//!
//! ```
//! use some_executor::task::{Task, ConfigurationBuilder};
//! use some_executor::hint::Hint;
//!
//! let config = ConfigurationBuilder::new()
//!     .hint(Hint::CPU)
//!     .build();
//!
//! let task = Task::without_notifications(
//!     "compute-hash".to_string(),
//!     config,
//!     async {
//!         // CPU-intensive hashing operation
//!         let mut result = 0u64;
//!         for i in 0..1_000_000 {
//!             result = result.wrapping_add(i);
//!         }
//!         result
//!     },
//! );
//! ```
//!
//! ## Marking an I/O-bound task
//!
//! ```
//! use some_executor::task::{Task, ConfigurationBuilder};
//! use some_executor::hint::Hint;
//!
//! let config = ConfigurationBuilder::new()
//!     .hint(Hint::IO)
//!     .build();
//!
//! let task = Task::without_notifications(
//!     "fetch-data".to_string(),
//!     config,
//!     async {
//!         // This task would typically await network or file operations
//!         // that spend most time waiting for I/O
//!         "fetched data"
//!     },
//! );
//! ```

/// Describes the expected runtime characteristics of a task.
///
/// `Hint` provides guidance to executors about how a task is likely to behave
/// during execution. Executors can use this information to optimize scheduling,
/// but they are not required to act on these hints.
///
/// # Variants
///
/// - [`Unknown`](Hint::Unknown): No information about task behavior (default)
/// - [`IO`](Hint::IO): Task is expected to yield frequently while waiting for I/O
/// - [`CPU`](Hint::CPU): Task is expected to perform intensive computation
///
/// # Examples
///
/// ```
/// use some_executor::hint::Hint;
/// use some_executor::task::{Task, ConfigurationBuilder};
///
/// // CPU-bound task
/// let cpu_config = ConfigurationBuilder::new()
///     .hint(Hint::CPU)
///     .build();
///
/// // I/O-bound task
/// let io_config = ConfigurationBuilder::new()
///     .hint(Hint::IO)
///     .build();
///
/// // Unknown (let executor decide)
/// let default_config = ConfigurationBuilder::new()
///     .hint(Hint::Unknown)
///     .build();
/// ```
///
/// # Non-exhaustive
///
/// This enum is marked `#[non_exhaustive]` to allow for future variants
/// without breaking existing code. Always include a catch-all pattern when
/// matching:
///
/// ```
/// use some_executor::hint::Hint;
///
/// fn describe_hint(hint: Hint) -> &'static str {
///     match hint {
///         Hint::Unknown => "unknown workload",
///         Hint::IO => "I/O-bound workload",
///         Hint::CPU => "CPU-bound workload",
///         _ => "unrecognized hint",
///     }
/// }
/// ```
#[non_exhaustive]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum Hint {
    /// No information about the task's expected behavior.
    ///
    /// Use this when the task's characteristics are unknown or when it has
    /// a mixed workload. This is the default hint and allows executors to
    /// apply their default scheduling policies.
    #[default]
    Unknown,

    /// The task is expected to spend most of its time yielded waiting for I/O.
    ///
    /// I/O-bound tasks typically:
    /// - Wait for network responses, disk operations, or timers
    /// - Yield frequently and quickly resume when I/O completes
    /// - Don't consume much CPU time relative to wall-clock time
    ///
    /// Executors may schedule I/O-bound tasks with less concern about blocking
    /// other work, as they naturally yield often.
    IO,

    /// The task is expected to spend most of its time computing.
    ///
    /// CPU-bound tasks typically:
    /// - Perform intensive calculations or data processing
    /// - May run for extended periods without yielding
    /// - Consume significant CPU time
    ///
    /// Executors may run CPU-bound tasks on dedicated threads or apply
    /// preemption to prevent them from starving other tasks.
    CPU,
}
