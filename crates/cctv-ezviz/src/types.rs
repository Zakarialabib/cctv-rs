//! Driver value types, reduced to the cctv device model.
//!
//! These are the shapes the driver hands back to the facade; they carry no
//! Ezviz wire detail the app would have to know.

/// A discovered / configured Ezviz camera, reduced to the cctv device model.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EzvizCamera {
    /// Unique identifier for the camera.
    pub id: String,
    /// Human-readable display name.
    pub label: String,
    /// LAN address of the camera, when it is reachable directly.
    pub ip: Option<String>,
    /// Hardware MAC address, when the platform reports it.
    pub mac: Option<String>,
    /// Firmware version string, when the platform reports it.
    pub firmware: Option<String>,
    /// Live-video stream URL, when one is available.
    pub stream_url: Option<String>,
    /// RTMP URL for the camera, when one is available.
    pub rtmp_url: Option<String>,
    /// Whether the camera supports pan, tilt and zoom.
    pub ptz: bool,
    /// Whether the camera supports on-board AI detection.
    pub ai: bool,
    /// Route the driver used to reach the camera.
    pub connectivity: Connectivity,
}

/// How the driver reached the camera.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Connectivity {
    /// Via the cloud (token + Ezviz account).
    Cloud,
    /// Direct LAN radio. The driver chooses this when the camera is reachable
    /// locally and the operator has pinned the cert.
    Direct,
}
