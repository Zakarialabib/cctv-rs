//! `cctv-rs` — the facade crate.
//!
//! The only crate that SignageMaster's `cctv.ts` talks to. Nothing in here is
//! vendor-specific: the app sees only the `cctv-core` device model and our
//! event bus.

#![forbid(unsafe_code)]

pub mod facade;

pub use cctv_core::CameraError;
pub use cctv_core::CameraHandle;
