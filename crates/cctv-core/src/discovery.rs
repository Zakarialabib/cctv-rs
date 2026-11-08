//! Device discovery abstraction: traits, types, and seams.
//!
//! §2.3: `Discoverable` allows drivers to implement their own discovery probes
//! without depending on `cctv-driver`.

use crate::prelude::*;
use crate::ProbeResult;
use core::time::Duration;

pub use core::net::SocketAddr;

/// One camera found by a probe sweep.
///
/// `id` is the stable identity discovery observed — the SSDP `USN`, or the
/// WS-Discovery endpoint address — and is what the fleet adopts the camera
/// under.  `service_url` is the device's control-service endpoint when the
/// protocol advertised one (a WS-Discovery `XAddr`), which becomes
/// `CameraConfig::service_url` on adoption.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-sync", derive(serde::Serialize, serde::Deserialize))]
pub struct DiscoveredCamera {
    /// Stable identity discovery observed.
    pub id: String,
    /// The device-model probe fields for this camera.
    pub probe: ProbeResult,
    /// Control-service URL the device advertised, if any.
    pub service_url: Option<String>,
}

impl DiscoveredCamera {
    /// Build a discovered camera from an identity and its probe fields.
    pub fn new(id: impl Into<String>, probe: ProbeResult) -> Self {
        Self {
            id: id.into(),
            probe,
            service_url: None,
        }
    }

    /// Attach a service URL to the discovered camera.
    pub fn with_service_url(mut self, service_url: impl Into<String>) -> Self {
        self.service_url = Some(service_url.into());
        self
    }
}

/// The datagram transport a sweep runs on.
///
/// Implemented for real UDP sockets and for test doubles, so sweep logic
/// is exercised without a network.
pub trait Datagram: Send + Sync {
    /// Send `request` to `group` and collect every datagram that arrives before
    /// `timeout` elapses.
    ///
    /// Must return whatever it has once the timeout passes rather than blocking
    /// indefinitely, and must never panic: an unreachable network yields an
    /// empty sweep, not an error.
    fn exchange(&self, group: SocketAddr, request: &[u8], timeout: Duration) -> Vec<Vec<u8>>;
}

/// A sweep that finds its protocol's devices on the local network.
///
/// Object-safe (`Send + Sync`, no generics) so facade/drivers can hold it behind
/// trait objects without needing driver-specific linkage.
pub trait Discoverable: Send + Sync {
    /// Sweep over an injected transport — the test seam, no sockets.
    fn discover_with(&self, net: &dyn Datagram) -> Vec<DiscoveredCamera>;

    /// Sweep the real network over UDP multicast or protocol-specific means.
    ///
    /// An unreachable network yields an empty sweep, never an error.
    fn discover(&self) -> Vec<DiscoveredCamera> {
        Vec::new()
    }
}
