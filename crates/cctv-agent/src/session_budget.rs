//! Session budgeting and concurrency management per camera.
//!
//! Cameras support a bounded number of concurrent RTSP sessions.
//! This module tracks active RTSP stream handles against each camera's
//! declared capabilities, rejecting requests with `CameraError::SessionLimitExceeded`
//! before network sockets are opened, and tearing down active sessions
//! when a `MaintenanceLease` commands `DisconnectAndYield`.

use std::collections::{HashMap, HashSet};

use cctv_core::{
    CameraCapabilitySet, CameraError, CameraId, LeaseMode, MaintenanceLease, StreamHandle,
};
use serde::{Deserialize, Serialize};

/// Operational capabilities of a camera, including concurrent stream limits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraCapabilities {
    /// Maximum concurrent RTSP streams supported by the device hardware/firmware.
    pub max_concurrent_rtsp_streams: u32,
}

impl CameraCapabilities {
    /// Create a new `CameraCapabilities` with the specified max stream limit.
    pub fn new(max_concurrent_rtsp_streams: u32) -> Self {
        Self {
            max_concurrent_rtsp_streams,
        }
    }
}

impl From<CameraCapabilitySet> for CameraCapabilities {
    fn from(caps: CameraCapabilitySet) -> Self {
        Self {
            max_concurrent_rtsp_streams: caps.max_concurrent_rtsp_streams as u32,
        }
    }
}

/// Tracks active RTSP streams per camera and enforces hardware concurrency limits.
#[derive(Debug, Default)]
pub struct SessionManager {
    active_streams: HashMap<CameraId, HashSet<StreamHandle>>,
    next_handle_id: u64,
}

impl SessionManager {
    /// Create a new, empty session manager.
    pub fn new() -> Self {
        Self {
            active_streams: HashMap::new(),
            next_handle_id: 1,
        }
    }

    /// Number of active RTSP streams currently held for a camera.
    pub fn active_count(&self, camera_id: &CameraId) -> usize {
        self.active_streams
            .get(camera_id)
            .map(|set| set.len())
            .unwrap_or(0)
    }

    /// Acquire a new RTSP stream handle against `capabilities.max_concurrent_rtsp_streams`.
    ///
    /// Returns `CameraError::SessionLimitExceeded` before opening a socket if the budget is full.
    pub fn acquire_session(
        &mut self,
        camera_id: &CameraId,
        capabilities: &CameraCapabilities,
    ) -> Result<StreamHandle, CameraError> {
        let current_count = self.active_count(camera_id) as u32;
        if current_count >= capabilities.max_concurrent_rtsp_streams {
            return Err(CameraError::SessionLimitExceeded);
        }

        let handle = StreamHandle::new(self.next_handle_id);
        self.next_handle_id += 1;

        self.active_streams
            .entry(camera_id.clone())
            .or_default()
            .insert(handle);

        Ok(handle)
    }

    /// Release an active RTSP stream handle, freeing the session slot for the camera.
    pub fn release_session(&mut self, camera_id: &CameraId, handle: StreamHandle) -> bool {
        if let Some(handles) = self.active_streams.get_mut(camera_id) {
            let removed = handles.remove(&handle);
            if handles.is_empty() {
                self.active_streams.remove(camera_id);
            }
            removed
        } else {
            false
        }
    }

    /// Apply a maintenance lease directive.
    ///
    /// When `MaintenanceLease` mode is `LeaseMode::DisconnectAndYield`, tears down active RTSP sessions
    /// to free hardware slots for Mobile or external clients, returning the drained handles.
    pub fn apply_maintenance_lease(
        &mut self,
        camera_id: &CameraId,
        lease: &MaintenanceLease,
    ) -> Vec<StreamHandle> {
        match lease.mode {
            LeaseMode::ObserveAndRelay => Vec::new(),
            LeaseMode::DisconnectAndYield => {
                if let Some(handles) = self.active_streams.remove(camera_id) {
                    handles.into_iter().collect()
                } else {
                    Vec::new()
                }
            }
        }
    }

    /// Apply a maintenance lease mode directly.
    pub fn apply_lease_mode(&mut self, camera_id: &CameraId, mode: LeaseMode) -> Vec<StreamHandle> {
        match mode {
            LeaseMode::ObserveAndRelay => Vec::new(),
            LeaseMode::DisconnectAndYield => {
                if let Some(handles) = self.active_streams.remove(camera_id) {
                    handles.into_iter().collect()
                } else {
                    Vec::new()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_enforces_max_concurrent_rtsp_streams() {
        let mut mgr = SessionManager::new();
        let cam = CameraId::from("cam-front-door");
        let caps = CameraCapabilities::new(2);

        let s1 = mgr.acquire_session(&cam, &caps).expect("first session ok");
        let s2 = mgr.acquire_session(&cam, &caps).expect("second session ok");
        assert_eq!(mgr.active_count(&cam), 2);

        // Third session exceeds budget before socket creation
        let err = mgr.acquire_session(&cam, &caps).unwrap_err();
        assert_eq!(err, CameraError::SessionLimitExceeded);

        // Releasing s1 frees a slot
        assert!(mgr.release_session(&cam, s1));
        assert_eq!(mgr.active_count(&cam), 1);

        let s3 = mgr
            .acquire_session(&cam, &caps)
            .expect("third session after release ok");
        assert_eq!(mgr.active_count(&cam), 2);

        assert!(mgr.release_session(&cam, s2));
        assert!(mgr.release_session(&cam, s3));
        assert_eq!(mgr.active_count(&cam), 0);
    }

    #[test]
    fn disconnect_and_yield_tears_down_active_rtsp_sessions() {
        let mut mgr = SessionManager::new();
        let cam = CameraId::from("cam-driveway");
        let caps = CameraCapabilities::new(3);

        let s1 = mgr.acquire_session(&cam, &caps).unwrap();
        let s2 = mgr.acquire_session(&cam, &caps).unwrap();

        assert_eq!(mgr.active_count(&cam), 2);

        // Yielding maintenance lease tears down sessions to free slots for Mobile
        let lease = MaintenanceLease::new(
            "operator-1",
            cam.clone(),
            999_999,
            LeaseMode::DisconnectAndYield,
        );
        let drained = mgr.apply_maintenance_lease(&cam, &lease);
        assert_eq!(drained.len(), 2);
        assert!(drained.contains(&s1));
        assert!(drained.contains(&s2));
        assert_eq!(mgr.active_count(&cam), 0);

        // Slots are immediately available again
        let s3 = mgr
            .acquire_session(&cam, &caps)
            .expect("can acquire after yield");
        assert_eq!(mgr.active_count(&cam), 1);
        assert!(mgr.release_session(&cam, s3));
    }

    #[test]
    fn retain_lease_does_not_disturb_sessions() {
        let mut mgr = SessionManager::new();
        let cam = CameraId::from("cam-office");
        let caps = CameraCapabilities::new(2);

        let _s1 = mgr.acquire_session(&cam, &caps).unwrap();
        let lease = MaintenanceLease::new(
            "operator-1",
            cam.clone(),
            999_999,
            LeaseMode::ObserveAndRelay,
        );
        let drained = mgr.apply_maintenance_lease(&cam, &lease);
        assert!(drained.is_empty());
        assert_eq!(mgr.active_count(&cam), 1);
    }
}
