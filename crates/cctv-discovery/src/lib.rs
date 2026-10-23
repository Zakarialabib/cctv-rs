//! Camera discovery — SSDP, multicast, USB probe.
//!
//! Pure discovery: no TLS, no vendor, no protocol. Emits discovered cameras as
//! cctv device-model items and emits a `CameraConnected` event for each.

pub mod probe;

pub use probe::*;
