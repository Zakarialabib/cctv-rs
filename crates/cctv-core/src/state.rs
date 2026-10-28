//! Multi-producer, single-consumer state: the "belief" layer a CMS embeds.
//!
//! Everything here is plain Rust with no async runtime.  Consumers hold an
//! immutable snapshot and can compare confidence.  The reconciler is the only
//! thing that mutates this state, via [`CameraState`].
//!
//! # State machine
//!
//! ```text
//! Off → Standby → On → Streaming → Recording
//! Off → Away          (firmware-level offline)
//! ```
//!
//! Stepping backwards cools confidence; stepping forwards warms it.  A
//! `probe` always refreshes the anchor before a transition.

use crate::prelude::*;

use crate::types::CameraType;
use serde::{Deserialize, Serialize};

/// One camera, as the fleet believes it to be right now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraState {
    /// Stable identity chosen by the caller (MAC, serial, IP, Ezviz ID).  The
    /// key that joins a `discover` result back to a device the app already
    /// knows.
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
    /// Confidence 0.0..1.0 in `state`.  Starts at 0.0 until the first probe.
    pub confidence: f32,
    /// Last probe result fields (device model, firmware, IP, SSID) — the raw
    /// trust anchor for `confidence`.
    pub last_probe: Option<ProbeResult>,
    /// Timestamps for state machine transitions (seconds since epoch).
    pub probe_at: Option<u64>,
    /// Time the camera was last seen online (seconds since epoch).
    pub online_at: Option<u64>,
    /// Time the camera was last seen offline (seconds since epoch).
    pub offline_at: Option<u64>,
    /// Time the camera was last armed (seconds since epoch).
    pub armed_at: Option<u64>,
    /// Time the camera last started recording (seconds since epoch).
    pub recording_at: Option<u64>,
    /// How many consecutive retries this camera has gone through.
    pub retry_count: u32,
    /// When the next retry should be attempted (seconds since epoch), if any.
    pub retry_after: Option<u64>,
}

/// The camera's desired state, set by the operator or a dashboard rule.
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
    /// Recovering: a retry the camera is expected to pass.
    Retrying,
}

/// What the camera is believed to be doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CameraStateCurrent {
    /// Powered off.
    Off,
    /// Powered but idle, waiting for work.
    Standby,
    /// On and active, not yet streaming.
    On,
    /// Streaming, but not necessarily recording.
    Streaming,
    /// Recording to storage.
    Recording,
    /// PTZ move in progress.
    PtzMoving,
    /// Away / offline (firmware-level).
    Away,
    /// Disconnected / hot-unplugged (Ghost Device audit state).
    Disconnected,
    /// Retrying a probe or stream after being offline.
    Retrying,
}

/// Raw trust anchor from a probe.  Used to update confidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeResult {
    /// Vendor name reported by the device.
    pub vendor: String,
    /// Model name reported by the device.
    pub model: String,
    /// Firmware version, if reported.
    pub firmware: Option<String>,
    /// Device IP address, if known.
    pub ip: Option<String>,
    /// Device MAC address, if known.
    pub mac: Option<String>,
    /// Primary stream URL, if advertised.
    pub stream_url: Option<String>,
    /// Whether the device reports PTZ support.
    pub ptz_supported: bool,
    /// Whether the device reports AI detection support.
    pub ai_supported: bool,
}

impl CameraState {
    /// Create a camera in its initial belief: offline, `Idle`, `Off` and
    /// confidence 0.0 until the first probe.
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
            retry_count: 0,
            retry_after: None,
        }
    }

    /// Apply a probe result and put the state on the life-cycle chain.  The
    /// probe result is kept as the trust anchor for confidence.
    pub fn apply_probe(
        &mut self,
        probe: ProbeResult,
        online: bool,
        current: CameraStateCurrent,
        confidence: f32,
    ) {
        self.last_probe = Some(probe);
        self.probe_at = Some(Self::now());
        self.update_confirmation(online, current, confidence);
    }

    /// Update online state + state + confidence from a probe.  Records the
    /// timestamp.  `confidence` is clamped to [0, 1].
    pub fn update_confirmation(
        &mut self,
        online: bool,
        state: CameraStateCurrent,
        confidence: f32,
    ) {
        self.online = online;
        self.state = state;
        self.confidence = confidence.clamp(0.0, 1.0);
        if online {
            self.online_at = Some(Self::now());
            // A camera that comes back online resets its retry counters.
            self.retry_count = 0;
            self.retry_after = None;
        } else {
            self.offline_at = Some(Self::now());
        }
        // Stepping onto the chain warms confidence.
        if online && state >= CameraStateCurrent::Standby {
            self.state = state;
        }
    }

    /// Move the live state forward or backward along the chain, warming or
    /// cooling confidence respectively.
    pub fn transition(&mut self, next: CameraStateCurrent) {
        let chain = [
            CameraStateCurrent::Disconnected,
            CameraStateCurrent::Off,
            CameraStateCurrent::Standby,
            CameraStateCurrent::On,
            CameraStateCurrent::Streaming,
            CameraStateCurrent::Recording,
        ];
        let at = chain.iter().position(|&s| s == self.state).unwrap_or(0);
        let next_at = chain.iter().position(|&s| s == next).unwrap_or(0);

        if next_at > at {
            self.confidence = (self.confidence + 0.1).min(1.0);
        } else {
            self.confidence *= 0.9;
        }
        self.state = next;
    }

    /// Set the desired state directly.
    pub fn set_desired(&mut self, desired: CameraDesired) {
        self.desired = desired;
    }

    /// Move the camera into the armed desired state.
    pub fn arm(&mut self) {
        self.desired = CameraDesired::Armed;
    }

    /// Move the camera into the recording desired state.
    pub fn record(&mut self) {
        self.desired = CameraDesired::Recording;
    }

    /// Move the camera into the disabled desired state.
    pub fn disable(&mut self) {
        self.desired = CameraDesired::Disabled;
    }

    /// Move the camera into the retrying (recovery) desired state.
    ///
    /// The reconciler grades an offline camera as `Retry`, and this tells the
    /// camera to keep trying — the wall always looks alive.  `retry_after` is
    /// set so the probe loop knows when to try again.
    pub fn retry(&mut self, retry_after: u64) {
        self.desired = CameraDesired::Retrying;
        self.state = CameraStateCurrent::Retrying;
        self.retry_count += 1;
        self.retry_after = Some(retry_after.max(Self::now()));
    }

    /// Has the camera been retried?
    pub fn is_retrying(&self) -> bool {
        self.desired == CameraDesired::Retrying || self.state == CameraStateCurrent::Retrying
    }

    /// The number of consecutive retries the camera has gone through.
    pub fn retry_count(&self) -> u32 {
        self.retry_count
    }

    /// When the next retry should be attempted, if any.
    pub fn retry_after(&self) -> Option<u64> {
        self.retry_after
    }

    /// Seconds since the Unix epoch.
    ///
    /// The contract has no clock — `core` has no `SystemTime` — so this reads one
    /// only when `std` is linked (the `testing` and `quirks` builds).  An
    /// `alloc`-only build stamps transitions with `0`: the state machine behaves
    /// identically, but a caller that needs real timestamps records them itself,
    /// which is exactly what `cctv-agent`'s audit trail does.
    #[cfg(any(feature = "testing", feature = "quirks"))]
    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// An `alloc`-only build has no clock; see the `std` variant above.
    #[cfg(not(any(feature = "testing", feature = "quirks")))]
    fn now() -> u64 {
        0
    }

    #[test]
    fn a_camera_state_new_initialises_retry_counters_to_zero() {
        let state = CameraState::new("cam".into(), "ACME".into(), CameraType::Consumer);
        assert_eq!(state.retry_count, 0);
        assert_eq!(state.retry_after, None);
        assert!(!state.is_retrying());
    }

    #[test]
    fn retry_stage_moves_the_state_and_increments_the_count() {
        let mut state = CameraState::new("cam".into(), "ACME".into(), CameraType::Consumer);
        state.retry(1_700_000_000);
        assert_eq!(state.desired, CameraDesired::Retrying);
        assert_eq!(state.state, CameraStateCurrent::Retrying);
        assert_eq!(state.retry_count, 1);
        assert_eq!(state.retry_after, Some(1_700_000_000));
    }

    #[test]
    fn an_online_probe_resets_retry_counters() {
        let mut state = CameraState::new("cam".into(), "ACME".into(), CameraType::Consumer);
        state.retry(1_700_000_000);
        assert!(state.is_retrying());
        state.update_confirmation(true, CameraStateCurrent::On, 1.0);
        assert!(!state.is_retrying());
        assert_eq!(state.retry_count, 0);
        assert_eq!(state.retry_after, None);
    }
}

