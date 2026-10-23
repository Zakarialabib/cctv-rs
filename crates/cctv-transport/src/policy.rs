//! TLS policy, data-driven.
//!
//! The reconciler swaps a loader at runtime per deployment (VPS mirror, office
//! player, kiosk). Policy is data: pinned certs, min TLS version, ALPN.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub enum TlsVersion {
    V1_2,
    V1_3,
}

#[derive(Debug, Clone)]
pub struct Policy {
    pub min_version: TlsVersion,
    pub pinned_certs: Vec<String>,
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
    pub fn strict_pinned(certs: Vec<String>) -> Self {
        Self {
            min_version: TlsVersion::V1_3,
            pinned_certs: certs,
            prefer_h2: true,
        }
    }
}
