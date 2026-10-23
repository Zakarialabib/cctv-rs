//! Fake backend: stand-ins for hardware so the device model, reconciler and
//! driver can be unit-tested without a camera. Also the conformance suite's
//! subject under test.
//!
//! The mock speaks the same `CameraError` taxonomy as the real drivers, so a
//! test can assert that a `Refused` from the real device is handled the same way
//! as a `Refused` emitted by the mock.

use crate::state::{CameraDesired, CameraState, CameraStateCurrent};

#[derive(Default)]
pub struct MockBackend {
    /// id -> camera state (in-memory).
    cameras: std::collections::BTreeMap<String, crate::CameraState>,
    /// Logs every started stream / configured command, so tests can assert order.
    pub log: Vec<String>,
}

impl MockBackend {
    pub fn new() -> Self {
        Self {
            cameras: std::collections::BTreeMap::new(),
            log: Vec::new(),
        }
    }

    /// Add a camera to the mock and return its handle.
    pub fn add(&mut self, id: String, vendor: String, ty: crate::CameraType) -> &mut crate::CameraState {
        let st = crate::CameraState::new(id, vendor, ty);
        self.cameras.insert(id, st);
        self.cameras.get_mut(&id).unwrap()
    }

    pub fn get(&self, id: &str) -> Option<&crate::CameraState> {
        self.cameras.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut crate::CameraState> {
        self.cameras.get_mut(id)
    }

    /// Simulate a probe result.
    pub fn probe(&mut self, id: &str, online: bool, current: crate::CameraStateCurrent, confidence: f32) {
        if let Some(st) = self.cameras.get_mut(id) {
            st.update_confirmation(online, current, confidence);
        }
    }
}

/// Build a mock camera state for conformance tests.
pub fn mock_camera(id: &str, vendor: &str) -> crate::CameraState {
    crate::CameraState::new(id.to_string(), vendor.to_string(), crate::CameraType::Consumer)
}
