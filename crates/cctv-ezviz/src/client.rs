//! Ezviz client facade.
//!
//! The single place the driver knows how the Ezviz Open API is wired. Public
//! operations are expressed in cctv device-model terms, not Ezviz terms.

use serde::Deserialize;

/// A discovered / configured Ezviz camera, reduced to the cctv device model.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EzvizCamera {
    pub id: String,
    pub label: String,
    pub ip: Option<String>,
    pub mac: Option<String>,
    pub firmware: Option<String>,
    pub stream_url: Option<String>,
    pub rtmp_url: Option<String>,
    pub ptz: bool,
    pub ai: bool,
    pub connectivity: Connectivity,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Connectivity {
    /// Via the cloud (token + Ezviz account).
    Cloud,
    /// Direct LAN radio. The driver chooses this when the camera is reachable
    /// locally and the operator has pinned the cert.
    Direct,
}

/// Stub — the real Ezviz Open API types live in `ezviz-openapi-rs`.
pub use ezviz_openapi::{Device, DeviceInfo};
