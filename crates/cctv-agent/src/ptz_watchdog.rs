//! PTZ software watchdog.
//!
//! Tracks active `PtzPulse` deadlines (`Instant::now() + Duration::from_millis(ttl_ms)`).
//! If a deadline expires without a renewal `PtzPulse` or explicit `PtzStop`,
//! the watchdog marks the camera as expired so that emergency PTZ stop can be dispatched.
//!
//! Deadlines are read from [`tokio::time::Instant`], the same clock the runner
//! sleeps on.  `std::time::Instant` would keep running while a paused test
//! advances virtual time, so a deadline armed on the wall clock could never be
//! observed to expire deterministically.

use std::collections::HashMap;
use std::time::Duration;

use tokio::time::Instant;

use cctv_core::{CameraId, ImperativeCameraCommand};
use cctv_driver::onvif::{build_continuous_move, build_stop};
use serde::{Deserialize, Serialize};

/// Heartbeat pulse keeping a PTZ continuous move alive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtzPulse {
    /// Camera target identifier.
    pub camera_id: CameraId,
    /// Pan velocity in `-1.0..=1.0`.
    #[serde(default)]
    pub pan: f32,
    /// Tilt velocity in `-1.0..=1.0`.
    #[serde(default)]
    pub tilt: f32,
    /// Zoom velocity in `-1.0..=1.0`.
    #[serde(default)]
    pub zoom: f32,
    /// Time-to-live for this pulse in milliseconds.
    pub ttl_ms: u64,
}

impl PtzPulse {
    /// Create a new PTZ heartbeat pulse with zero velocities.
    pub fn new(camera_id: impl Into<CameraId>, ttl_ms: u64) -> Self {
        Self {
            camera_id: camera_id.into(),
            pan: 0.0,
            tilt: 0.0,
            zoom: 0.0,
            ttl_ms,
        }
    }

    /// Create a new PTZ pulse with directional velocities.
    pub fn with_velocities(
        camera_id: impl Into<CameraId>,
        pan: f32,
        tilt: f32,
        zoom: f32,
        ttl_ms: u64,
    ) -> Self {
        Self {
            camera_id: camera_id.into(),
            pan,
            tilt,
            zoom,
            ttl_ms,
        }
    }

    /// Convert to the canonical `ImperativeCameraCommand::PtzPulse`.
    pub fn to_imperative_command(&self) -> Result<ImperativeCameraCommand, cctv_core::CoreError> {
        ImperativeCameraCommand::ptz_pulse(self.pan, self.tilt, self.zoom, self.ttl_ms as u16)
    }

    /// Build the ONVIF `ContinuousMove` SOAP fragment for this pulse.
    pub fn build_continuous_move_soap(&self, profile_token: &str) -> String {
        build_continuous_move(profile_token, self.pan, self.tilt, self.zoom, None)
    }
}

/// Explicit command to stop PTZ movement and disarm the watchdog for a camera.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtzStop {
    /// Camera target identifier.
    pub camera_id: CameraId,
}

impl PtzStop {
    /// Create an explicit stop message.
    pub fn new(camera_id: impl Into<CameraId>) -> Self {
        Self {
            camera_id: camera_id.into(),
        }
    }

    /// Convert to the canonical `ImperativeCameraCommand::PtzStop`.
    pub fn to_imperative_command(&self) -> ImperativeCameraCommand {
        ImperativeCameraCommand::PtzStop
    }

    /// Build the ONVIF `Stop` SOAP fragment.
    pub fn build_stop_soap(&self, profile_token: &str) -> String {
        build_stop(profile_token, true, true)
    }
}

/// Software watchdog tracking PTZ pulse deadlines per camera.
#[derive(Debug, Default)]
pub struct PtzWatchdog {
    deadlines: HashMap<CameraId, Instant>,
}

impl PtzWatchdog {
    /// Create a new, empty PTZ watchdog.
    pub fn new() -> Self {
        Self {
            deadlines: HashMap::new(),
        }
    }

    /// Arm or renew the PTZ deadline for a camera.
    pub fn pulse(&mut self, pulse: &PtzPulse) {
        let deadline = Instant::now() + Duration::from_millis(pulse.ttl_ms);
        self.deadlines.insert(pulse.camera_id.clone(), deadline);
    }

    /// Clear the PTZ deadline for a camera upon explicit stop.
    pub fn stop(&mut self, stop: &PtzStop) {
        self.deadlines.remove(&stop.camera_id);
    }

    /// Earliest active deadline across all cameras, if any.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.deadlines.values().copied().min()
    }

    /// Check for and collect any camera IDs whose deadline has expired as of `now`.
    ///
    /// Expired entries are removed from tracking.
    pub fn check_expired_at(&mut self, now: Instant) -> Vec<CameraId> {
        let mut expired = Vec::new();
        self.deadlines.retain(|camera_id, &mut deadline| {
            if now >= deadline {
                expired.push(camera_id.clone());
                false
            } else {
                true
            }
        });
        expired
    }

    /// Check for and collect any camera IDs whose deadline has expired right now.
    pub fn check_expired(&mut self) -> Vec<CameraId> {
        self.check_expired_at(Instant::now())
    }

    /// Check if a specific camera has an active PTZ deadline.
    pub fn is_active(&self, camera_id: &CameraId) -> bool {
        self.deadlines.contains_key(camera_id)
    }

    /// Number of cameras currently under PTZ watchdog monitoring.
    pub fn active_count(&self) -> usize {
        self.deadlines.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_arms_and_renews_deadline() {
        let mut watchdog = PtzWatchdog::new();
        let cam = CameraId::from("cam-ptz-1");

        assert_eq!(watchdog.active_count(), 0);
        watchdog.pulse(&PtzPulse::new(cam.clone(), 500));
        assert_eq!(watchdog.active_count(), 1);
        assert!(watchdog.is_active(&cam));
        assert!(watchdog.next_deadline().is_some());

        // Renewing extends deadline
        let earlier = watchdog.next_deadline().unwrap();
        watchdog.pulse(&PtzPulse::new(cam.clone(), 1000));
        let later = watchdog.next_deadline().unwrap();
        assert!(later >= earlier);
    }

    #[test]
    fn stop_disarms_deadline() {
        let mut watchdog = PtzWatchdog::new();
        let cam = CameraId::from("cam-ptz-2");

        watchdog.pulse(&PtzPulse::new(cam.clone(), 500));
        assert!(watchdog.is_active(&cam));

        watchdog.stop(&PtzStop::new(cam.clone()));
        assert!(!watchdog.is_active(&cam));
        assert_eq!(watchdog.active_count(), 0);
        assert!(watchdog.next_deadline().is_none());
    }

    #[test]
    fn check_expired_identifies_and_cleans_up_expired_deadlines() {
        let mut watchdog = PtzWatchdog::new();
        let cam1 = CameraId::from("cam-1");
        let cam2 = CameraId::from("cam-2");

        let now = Instant::now();
        watchdog
            .deadlines
            .insert(cam1.clone(), now + Duration::from_millis(50));
        watchdog
            .deadlines
            .insert(cam2.clone(), now + Duration::from_millis(200));

        let expired = watchdog.check_expired_at(now + Duration::from_millis(100));
        assert_eq!(expired, vec![cam1]);
        assert_eq!(watchdog.active_count(), 1);
        assert!(watchdog.is_active(&cam2));

        let expired2 = watchdog.check_expired_at(now + Duration::from_millis(300));
        assert_eq!(expired2, vec![cam2]);
        assert_eq!(watchdog.active_count(), 0);
    }
}
