//! cctv-core — the camera device model (no tokio, no protocol).
//!
//! Everything a frontend or a reconciler needs to reason about a camera without
//! knowing *who* makes it. Protocols arrive through the driver crates.

pub mod commands;
pub mod error;
pub mod quirks;
pub mod state;
pub mod types;
