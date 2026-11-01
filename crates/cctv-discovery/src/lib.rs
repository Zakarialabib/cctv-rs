//! Camera discovery — SSDP `M-SEARCH`, WS-Discovery `Probe`, SADP XML inquiries, and bounded sweeps.
//!
//! Pure discovery: no TLS, no vendor, no protocol.  A sweep finds cameras and
//! hands each one back as a [`DiscoveredCamera`] — the stable identity discovery
//! observed plus a [`cctv_core::ProbeResult`] — so the fleet can adopt a device
//! without knowing the vendor's protocol.  This crate never authenticates and
//! never controls a camera.
//!
//! The wire formats are pure functions ([`ssdp`], [`wsd`], [`multicast`]); the UDP
//! exchange is behind the [`Datagram`] trait, so a sweep runs against a real socket or
//! against a test double, and an unreachable network yields an empty sweep
//! rather than a fabricated device.
//!
//! USB cameras are enumerated by the **application**: a USB camera is a kernel /
//! hardware concern, not a protocol, so this crate does not touch the USB bus.

#![forbid(unsafe_code)]

pub mod clock_probe;
pub mod multicast;
pub mod probe;
pub mod ssdp;
#[cfg(feature = "mobile")]
pub mod sweep;
pub mod wsd;

pub use clock_probe::{probe_onvif_clock, DiscoveryError};
pub use multicast::{
    camera_from_sadp, parse_sadp_reply, sadp_probe, scan_multicast, scan_multicast_with,
    DiscoveryOutcome, SadpReply, MULTICAST_IP, SADP_PORT, WSD_PORT,
};
pub use probe::{Datagram, DiscoveredCamera, Probe, UdpTransport};
