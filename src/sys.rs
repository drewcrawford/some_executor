// SPDX-License-Identifier: MIT OR Apache-2.0

//! Platform-specific implementations

// Time-related types
#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;

#[cfg(target_arch = "wasm32")]
pub use web_time::Instant;

// Platform-specific executor implementations
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod stdlib;

#[cfg(target_arch = "wasm32")]
pub(crate) mod wasm;

// Re-export the appropriate implementation
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use stdlib::*;

#[cfg(target_arch = "wasm32")]
pub(crate) use wasm::*;
