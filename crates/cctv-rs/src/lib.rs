//! `cctv` — the facade crate.
//!
//! The only crate that SignageMaster's `cctv.ts` talks to. Nothing in here is
//! vendor-specific: the app sees only the `cctv-core` device model and our
//! event bus.

pub mod facade;
pub mod types;

pub use cctv_core::CameraHandle;
pub use cctv_core::CameraError;
