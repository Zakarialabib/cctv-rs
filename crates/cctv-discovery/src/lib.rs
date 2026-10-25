//! Camera discovery — the SSDP `M-SEARCH` and WS-Discovery `Probe` sweeps.
//!
//! Pure discovery: no TLS, no vendor, no protocol.  A sweep finds cameras and
//! hands each one back as a [`DiscoveredCamera`] — the stable identity discovery
//! observed plus a [`cctv_core::ProbeResult`] — so the fleet can adopt a device
//! without knowing the vendor's protocol.  This crate never authenticates and
//! never controls a camera.
//!
//! The wire formats are pure functions ([`ssdp`], [`wsd`]); the UDP exchange is
//! behind the [`Datagram`] trait, so a sweep runs against a real socket or
//! against a test double, and an unreachable network yields an empty sweep
//! rather than a fabricated device.
//!
//! USB cameras are enumerated by the **application**: a USB camera is a kernel /
//! hardware concern, not a protocol, so this crate does not touch the USB bus.

#![forbid(unsafe_code)]

pub mod probe;
pub mod ssdp;
pub mod wsd;

pub use probe::{Datagram, DiscoveredCamera, Probe, UdpTransport};
