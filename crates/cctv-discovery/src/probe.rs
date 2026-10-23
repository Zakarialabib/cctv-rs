//! LAN probe surface: SSDP, multicast and the USB bus.
//!
//! Discovery is unauthenticated and read-only.  A probe finds cameras and hands
//! each one back as a [`ProbeResult`] — the `cctv-core` device-model item — plus
//! the stable identity discovery observed, so the fleet can adopt it without
//! knowing the vendor's protocol.

use cctv_core::ProbeResult;

/// One camera found by a probe sweep.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredCamera {
    /// Stable identity discovery observed (MAC when advertised, else IP).
    pub id: String,
    /// The device-model probe fields for this camera.
    pub probe: ProbeResult,
}

/// A discovery probe.  Stateless: each [`Probe::scan`] is one sweep.
#[derive(Debug, Default, Clone, Copy)]
pub struct Probe;

impl Probe {
    /// Create a probe.
    pub fn new() -> Self {
        Self
    }

    /// Sweep the network / USB bus and return every camera found.
    ///
    /// The transports (SSDP `M-SEARCH`, multicast, USB enumeration) land in this
    /// crate; until they are wired the sweep finds nothing.
    pub fn scan(&self) -> Vec<DiscoveredCamera> {
        Vec::new()
    }
}
