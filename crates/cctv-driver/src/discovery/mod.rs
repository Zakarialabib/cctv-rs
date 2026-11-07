//! Camera discovery — SSDP `M-SEARCH`, WS-Discovery `Probe`, SADP XML inquiries, and bounded sweeps.
//!
//! §2.3: Vendor-neutral discovery implementation folded into `cctv-driver`.
//! Wire formats are pure functions ([`ssdp`], [`wsd`], [`multicast`]); the UDP
//! exchange is behind the [`Datagram`] trait, so a sweep runs against a real socket or
//! against a test double, and an unreachable network yields an empty sweep
//! rather than a fabricated device.

pub mod clock_probe;
pub mod multicast;
pub mod probe;
pub mod ssdp;
#[cfg(feature = "mobile")]
pub mod sweep;
pub mod wsd;

pub use cctv_core::{Datagram, Discoverable, DiscoveredCamera};
pub use clock_probe::{probe_onvif_clock, DiscoveryError};
pub use multicast::{
    camera_from_sadp, parse_sadp_reply, sadp_probe, scan_multicast, scan_multicast_with,
    DiscoveryOutcome, SadpReply, MULTICAST_IP, SADP_PORT, WSD_PORT,
};
pub use probe::{camera_from_probe_match, message_id, Probe, UdpTransport};
#[cfg(feature = "mobile")]
pub use sweep::{
    cameras_from_sweep, probe_unicast_sadp, sweep_host, sweep_ips, HostProbeResult,
    MAX_CONCURRENCY, PROBE_TIMEOUT, SWEEP_PORTS,
};
