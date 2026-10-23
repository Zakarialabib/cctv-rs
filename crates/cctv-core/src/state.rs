//! Multi-producer, single-consumer state: the "belief" layer a CMS embeds.
//!
//! Everything here is plain Rust with no async runtime. Consumers hold an
//! immutable snapshot and can compare confidence. The reconciler is the only
//! thing that mutates this state, via [`State::apply`].

use crate::CameraType;
use serde::{Deserialize, Serialize};

/// One camera, as the fleet believes it to be right now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraState {
    /// Stable identity chosen by the caller (MAC, serial, IP, Ezviz ID). The key
    /// that joins a `discover` result back to a device the app already knows.
    pub id: String,
    /// Vendor + model as reported by discovery / config.
    pub vendor: String,
    /// The `CameraType` of this device.
    pub ty: CameraType,
    /// Whether the camera is believed to be reachable.
    pub online: bool,
    /// Requested desired state (set by the operator or the reconciler).
    pub desired: CameraDesired,
    /// Current belief: what the device is thought to be doing.
    pub state: CameraStateCurrent,
    /// Confidence 0.0..1.0 in `state`. Starts at 0.0 until the first probe.
    pub confidence: f32,
    /// Last probe result fields (device model, firmware, IP, SSID) — the raw
    /// trust anchor for `confidence`.
    pub last_probe: Option<ProbeResult>,
    /// Timestamps for the_state machine transitions (seconds since epoch).
    pub probe_at: Option<u64>,
    pub online_at: Option<u64>,
    pub offline_at: Option<u64>,
    pub armed_at: Option<u64>,
    pub recording_at: Option<u64>,
}

/// The desired state, set by the operator or a dashboard rule.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CameraDesired {
    /// Idle: stream on, but nothing scripted.
    Idle,
    /// Armed: live + pending events, no recording by default.
    Armed,
    /// Recording: stream + encode + rotate storage.
    Recording,
    /// Disabled / deleted from the fleet.
    Disabled,
}

/// What the camera is believed to be doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CameraStateCurrent {
    Off,
    Standby,
    On,
    /// Streaming, but not necessarily recording.
    Streaming,
    Recording,
    /// PTZ move in progress.
    PtzMoving,
    /// Away / offline (firmware-level).
    Away,
}

/// Raw trust anchor from a probe. Used to update confidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeResult {
    pub vendor: String,
    pub model: String,
    pub firmware: Option<String>,
    pub ip: Option<String>,
    pub mac: Option<String>,
    pub stream_url: Option<String>,
    pub ptz_supported: bool,
    pub ai_supported: bool,
}

impl CameraState {
    pub fn new(id: String, vendor: String, ty: CameraType) -> Self {
        Self {
            id,
            vendor,
            ty,
            online: false,
            desired: CameraDesired::Idle,
            state: CameraStateCurrent::Off,
            confidence: 0.0,
            last_probe: None,
            probe_at: None,
            online_at: None,
            offline_at: None,
            armed_at: None,
            recording_at: None,
        }
    }

    pub fn update_confirmation(&mut self, online: bool, state: CameraStateCurrent, confidence: f32) {
        self.online = online;
        self.state = state;
        self.confidence = confidence.clamp(0.0, 1.0);
        if online {
            self.online_at = Some(expensive_now());
        } else {
            self.offline_at = Some(expensive_now());
        }
    }

    pub fn set_desired(&mut self, desired: CameraDesired) {
        self.desired = desired;
    }
}

fn expensive_now() -> u64 {
    // Zero-dep stand-in. Replace with `std::time::SystemTime::now()` in production.
    0
}
