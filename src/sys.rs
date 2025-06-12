// SPDX-License-Identifier: MIT OR Apache-2.0

#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;

#[cfg(target_arch = "wasm32")]
pub use web_time::Instant;
