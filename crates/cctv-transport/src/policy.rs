//! TLS policy, data-driven.
//!
//! The reconciler swaps a loader at runtime per deployment (VPS mirror, office
//! player, kiosk). Policy is data: pinned certs, min TLS version, ALPN.

use serde::Deserialize;

/// Minimum TLS protocol version a policy will accept.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub enum TlsVersion {
    /// TLS version 1.2.
    V1_2,
    /// TLS version 1.3, the default minimum.
    V1_3,
}

/// TLS parameters used when connecting to a camera.
#[derive(Debug, Clone)]
pub struct Policy {
    /// Lowest TLS version the client will negotiate.
    pub min_version: TlsVersion,
    /// Certificates the peer must present; an empty list disables pinning.
    pub pinned_certs: Vec<String>,
    /// Whether to advertise HTTP/2 over ALPN when the peer supports it.
    pub prefer_h2: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            min_version: TlsVersion::V1_3,
            pinned_certs: vec![],
            prefer_h2: true,
        }
    }
}

impl Policy {
    /// Build a policy that pins `certs` at TLS 1.3 with HTTP/2 preferred.
    pub fn strict_pinned(certs: Vec<String>) -> Self {
        Self {
            min_version: TlsVersion::V1_3,
            pinned_certs: certs,
            prefer_h2: true,
        }
    }
}
