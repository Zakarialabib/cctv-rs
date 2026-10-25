//! Driver value types, reduced to the cctv device model.
//!
//! These are the shapes the driver hands back to the facade; they carry no
//! Ezviz wire detail the application would have to know.

/// A discovered / configured Ezviz camera, reduced to the cctv device model.
///
/// `stream_url` and `rtmp_url` are normally `None` on a discovery result: the
/// platform issues play addresses that expire, so they are resolved on demand
/// through [`crate::EzvizCameraBackend`] rather than cached on the camera.
///
/// `ptz` and `ai` are `false` here because the platform's device list does not
/// report capability — it is settled by the per-camera probe.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EzvizCamera {
    /// Unique identifier for the camera (channel-qualified when the device has
    /// more than one channel).
    pub id: String,
    /// Human-readable display name.
    pub label: String,
    /// LAN address of the camera, when it is reachable directly.
    pub ip: Option<String>,
    /// Hardware MAC address, when the platform reports it.
    pub mac: Option<String>,
    /// Firmware version string, when the platform reports it.
    pub firmware: Option<String>,
    /// Live-video stream URL, when one is already known.
    pub stream_url: Option<String>,
    /// RTMP URL for the camera, when one is already known.
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

/// One alarm, reduced to the device model.
///
/// The platform reports alarm *type codes* rather than names; `kind` keeps the
/// code so the application can label it, and the driver never invents a name.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Alarm {
    /// Platform alarm id.
    pub id: String,
    /// Device that raised it.
    pub device_serial: String,
    /// Channel that raised it (1 when the platform does not report one).
    pub channel: i32,
    /// Platform alarm-type code.
    pub kind: i32,
    /// When the alarm fired, in Unix **milliseconds**.
    pub at_ms: i64,
    /// Whether the operator has already read it.
    pub read: bool,
    /// Alarm snapshot URL, when the platform supplied one.
    pub snapshot_url: Option<String>,
}
