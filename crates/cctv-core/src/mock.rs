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

/// In-memory [`CameraBackend`](crate::backend::CameraBackend): one fake camera
/// with a stream, a snapshot, PTZ and device-side presets.
///
/// It is the reference subject of the conformance suite, and a stand-in for a
/// real driver in application tests.
pub struct MockCamera {
    presets: std::sync::Mutex<Vec<crate::Preset>>,
    next_preset: std::sync::atomic::AtomicU32,
    /// Every PTZ command received, in order.
    pub ptz_log: std::sync::Mutex<Vec<crate::backend::PtzCommand>>,
}

impl MockCamera {
    /// A fake camera with no presets.
    pub fn new() -> Self {
        Self {
            presets: std::sync::Mutex::new(Vec::new()),
            next_preset: std::sync::atomic::AtomicU32::new(1),
            ptz_log: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl Default for MockCamera {
    fn default() -> Self {
        Self::new()
    }
}

fn poisoned() -> crate::CameraError {
    crate::CameraError::Other("mock state lock poisoned".into())
}

impl crate::backend::CameraBackend for MockCamera {
    fn driver(&self) -> &'static str {
        "mock"
    }

    fn probe(&self) -> crate::backend::BoxFuture<'_, Result<ProbeResult, crate::CameraError>> {
        Box::pin(async {
            Ok(ProbeResult {
                vendor: "Mock".into(),
                model: "MockCam".into(),
                firmware: Some("1.0".into()),
                ip: Some("127.0.0.1".into()),
                mac: None,
                stream_url: Some("rtsp://127.0.0.1/main".into()),
                ptz_supported: true,
                ai_supported: false,
            })
        })
    }

    fn stream(
        &self,
        quality: crate::backend::StreamQuality,
    ) -> crate::backend::BoxFuture<'_, Result<crate::backend::StreamInfo, crate::CameraError>> {
        Box::pin(async move {
            let path = match quality {
                crate::backend::StreamQuality::Main => "main",
                crate::backend::StreamQuality::Sub => "sub",
            };
            Ok(crate::backend::StreamInfo {
                uri: format!("rtsp://127.0.0.1/{path}"),
                profile: path.into(),
                quality,
                width: None,
                height: None,
                codec: Some("H264".into()),
            })
        })
    }

    fn snapshot(
        &self,
    ) -> crate::backend::BoxFuture<'_, Result<crate::backend::Snapshot, crate::CameraError>> {
        Box::pin(async {
            Ok(crate::backend::Snapshot::from_bytes(
                vec![0xFF, 0xD8, 0xFF, 0xD9],
                "image/jpeg",
            ))
        })
    }

    fn ptz(
        &self,
        command: crate::backend::PtzCommand,
    ) -> crate::backend::BoxFuture<'_, Result<(), crate::CameraError>> {
        Box::pin(async move {
            self.ptz_log.lock().map_err(|_| poisoned())?.push(command);
            Ok(())
        })
    }

    fn presets(
        &self,
    ) -> crate::backend::BoxFuture<'_, Result<Vec<crate::Preset>, crate::CameraError>> {
        Box::pin(async { Ok(self.presets.lock().map_err(|_| poisoned())?.clone()) })
    }

    fn save_preset<'a>(
        &'a self,
        name: &'a str,
        id: Option<&'a str>,
    ) -> crate::backend::BoxFuture<'a, Result<crate::Preset, crate::CameraError>> {
        Box::pin(async move {
            let id = match id {
                Some(id) => id.to_string(),
                None => self
                    .next_preset
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                    .to_string(),
            };
            let preset = crate::Preset {
                id: id.clone(),
                name: name.to_string(),
                pan: 0.0,
                tilt: 0.0,
                zoom: 0.0,
                config: serde_json::Value::Null,
            };
            let mut all = self.presets.lock().map_err(|_| poisoned())?;
            all.retain(|p| p.id != id);
            all.push(preset.clone());
            Ok(preset)
        })
    }

    fn goto_preset<'a>(
        &'a self,
        id: &'a str,
    ) -> crate::backend::BoxFuture<'a, Result<(), crate::CameraError>> {
        Box::pin(async move {
            let all = self.presets.lock().map_err(|_| poisoned())?;
            if all.iter().any(|p| p.id == id) {
                Ok(())
            } else {
                Err(crate::CameraError::Other(format!("no preset {id}")))
            }
        })
    }

    fn delete_preset<'a>(
        &'a self,
        id: &'a str,
    ) -> crate::backend::BoxFuture<'a, Result<(), crate::CameraError>> {
        Box::pin(async move {
            self.presets
                .lock()
                .map_err(|_| poisoned())?
                .retain(|p| p.id != id);
            Ok(())
        })
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

    /// Minimal `std`-only executor: the mock's futures never wait on I/O.
    fn block_on<F: std::future::Future>(fut: F) -> F::Output {
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        let mut fut = std::pin::pin!(fut);
        loop {
            if let std::task::Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn mock_camera_passes_the_conformance_suite() {
        let cam = super::MockCamera::new();
        let report = block_on(crate::conformance::run(
            &cam,
            crate::conformance::Expectations::FULL,
        ));
        assert!(report.is_ok(), "{report}");
        assert_eq!(report.passed.len(), 7);
    }

    struct Bare;

    impl crate::backend::CameraBackend for Bare {
        fn driver(&self) -> &'static str {
            "bare"
        }
        fn probe(
            &self,
        ) -> crate::backend::BoxFuture<'_, Result<crate::ProbeResult, crate::CameraError>> {
            Box::pin(async { super::MockCamera::new().probe().await })
        }
        fn stream(
            &self,
            q: crate::backend::StreamQuality,
        ) -> crate::backend::BoxFuture<'_, Result<crate::backend::StreamInfo, crate::CameraError>>
        {
            Box::pin(async move { super::MockCamera::new().stream(q).await })
        }
    }

    #[test]
    fn a_backend_that_supports_nothing_fails_with_typed_errors() {
        let none = crate::conformance::Expectations {
            snapshot: false,
            ptz: false,
            presets: false,
        };
        let report = block_on(crate::conformance::run(&Bare, none));
        assert!(report.is_ok(), "{report}");
        // Claiming what the backend lacks is caught and reported, not panicked on.
        let full = block_on(crate::conformance::run(
            &Bare,
            crate::conformance::Expectations::FULL,
        ));
        assert_eq!(full.failures.len(), 3, "{full}");
    }

    #[test]
    fn disabled_desired_form() {
        let b = MockBackend::new();
        b.conformance_disabled_desired_form();
    }
}
