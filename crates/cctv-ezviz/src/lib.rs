//! Ezviz Open Platform driver for cctv-rs.
//!
//! Wraps [`ezviz_openapi`] with the cctv device-model surface. One crate per
//! vendor; this is the Ezviz one.
//!
//! The Ezviz IP camera's proprietary PC Open SDK is enabled with the `pc-sdk` feature.

pub mod client;
pub mod camera;
pub mod types;

pub use ezviz_openapi::*;
