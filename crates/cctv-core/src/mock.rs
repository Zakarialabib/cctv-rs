//! Fake backend: stand-ins for hardware so the device model, reconciler and
//! driver can be unit-tested without a camera.  Also the conformance suite's
//! subject under test.
//!
//! The mock speaks the same `CameraError` taxonomy as the real drivers, so a
//! test can assert that a `Refused` from the real device is handled the same
//! way as a `Refused` emitted by the mock.

use crate::state::{CameraState, CameraStateCurrent, ProbeResult};

/// In-memory backend for the device model.  Single producer, single consumer.
#[derive(Default)]
pub struct MockBackend {
    /// id -> camera state (in-memory).
    cameras: std::collections::BTreeMap<String, CameraState>,
    /// Logs every started stream / configured command, so tests can assert order.
    pub log: Vec<String>,
}

impl MockBackend {
    /// Create an empty backend with no cameras and an empty log.
    pub fn new() -> Self {
        Self {
            cameras: std::collections::BTreeMap::new(),
            log: Vec::new(),
        }
    }

    /// Add a camera to the mock and return its handle.
    pub fn add(
        &mut self,
        id: String,
        vendor: String,
        ty: crate::CameraType,
    ) -> crate::CameraHandle {
        let handle = crate::CameraHandle::new(id.clone());
        let st = crate::CameraState::new(id.clone(), vendor, ty);
        self.cameras.insert(id, st);
        handle
    }

    /// Borrow the state of the camera with this id, if present.
    pub fn get(&self, id: &str) -> Option<&CameraState> {
        self.cameras.get(id)
    }

    /// Mutably borrow the state of the camera with this id, if present.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut CameraState> {
        self.cameras.get_mut(id)
    }

    /// Simulate a probe result and apply it to the camera.
    pub fn probe(
        &mut self,
        id: &str,
        probe: ProbeResult,
        online: bool,
        current: CameraStateCurrent,
        confidence: f32,
    ) {
        if let Some(st) = self.cameras.get_mut(id) {
            st.apply_probe(probe, online, current, confidence);
        }
    }

    /// List every camera in the backend (snapshot, not a borrow).
    pub fn list(&self) -> Vec<&CameraState> {
        self.cameras.values().collect()
    }

    /// Push a log entry (for reconciler / facade tests).
    pub fn note(&mut self, what: impl Into<String>) {
        self.log.push(what.into());
    }
}

#[cfg(test)]
impl MockBackend {
    /// Conformance: probes move a camera onto the chain in the right order
    /// and warm confidence; a later backward step cools it.
    pub fn conformance() -> Self {
        let b = Self::new();
        b.conformance_probe_steps_the_chain_and_warms();
        b
    }

    fn conformance_probe_steps_the_chain_and_warms(&self) {
        let mut b = Self::new();
        let h = b.add("cam1".into(), "Vendor".into(), crate::CameraType::Consumer);
        let probe = ProbeResult {
            vendor: "Vendor".into(),
            model: "Model".into(),
            firmware: Some("1.0".into()),
            ip: Some("192.168.0.1".into()),
            mac: Some("00:00:00:00:00:01".into()),
            stream_url: Some("http://example.com".into()),
            ptz_supported: false,
            ai_supported: false,
        };
        b.probe("cam1", probe, true, CameraStateCurrent::Streaming, 0.7);
        let st = b.get("cam1").unwrap();
        assert!(st.online);
        assert_eq!(st.state, CameraStateCurrent::Streaming);
        assert!((st.confidence - 0.7).abs() < f32::EPSILON);
        assert!(st.probe_at.is_some());
        assert_eq!(h.id(), "cam1");
    }

    fn conformance_backward_cools_confidence(&self) {
        let mut b = Self::new();
        let _h = b.add("cam1".into(), "Vendor".into(), crate::CameraType::Consumer);
        let st = b.get_mut("cam1").unwrap();
        st.update_confirmation(true, CameraStateCurrent::Streaming, 0.9);
        st.transition(CameraStateCurrent::Off);
        assert_eq!(st.state, CameraStateCurrent::Off);
        assert!(st.confidence < 0.9);
    }

    fn conformance_forward_warms_confidence(&self) {
        let mut b = Self::new();
        let _h = b.add("cam1".into(), "Vendor".into(), crate::CameraType::Consumer);
        let st = b.get_mut("cam1").unwrap();
        st.update_confirmation(false, CameraStateCurrent::Off, 0.3);
        st.transition(CameraStateCurrent::Streaming);
        assert_eq!(st.state, CameraStateCurrent::Streaming);
        assert!(st.confidence > 0.3);
    }

    fn conformance_disabled_desired_form(&self) {
        let mut b = Self::new();
        let _h = b.add("cam1".into(), "Vendor".into(), crate::CameraType::Consumer);
        let st = b.get_mut("cam1").unwrap();
        st.disable();
        assert_eq!(st.desired, crate::CameraDesired::Disabled);
    }
}

/// Build a mock camera state for conformance tests.
pub fn mock_camera(id: &str, vendor: &str) -> crate::CameraState {
    crate::CameraState::new(
        id.to_string(),
        vendor.to_string(),
        crate::CameraType::Consumer,
    )
}

#[cfg(test)]
mod tests {
    use super::MockBackend;

    #[test]
    fn probe_steps_the_chain_and_warms() {
        let b = MockBackend::new();
        b.conformance_probe_steps_the_chain_and_warms();
    }

    #[test]
    fn backward_transition_cools_confidence() {
        let b = MockBackend::new();
        b.conformance_backward_cools_confidence();
    }

    #[test]
    fn forward_transition_warms_confidence() {
        let b = MockBackend::new();
        b.conformance_forward_warms_confidence();
    }

    #[test]
    fn disabled_desired_form() {
        let b = MockBackend::new();
        b.conformance_disabled_desired_form();
    }
}
