//! Mock driver adapter implementation for testing and simulation.

use async_trait::async_trait;
use cctv_core::{CameraBackend, CameraConfig, CameraError, Capability, CapabilitySet, ProbeResult};

use crate::DriverAdapter;

/// In-memory mock camera backend created by [`MockDriverAdapter`].
#[derive(Debug, Clone)]
pub struct MockBackendHandle {
    key: &'static str,
    caps: CapabilitySet,
}

impl MockBackendHandle {
    /// Create a new mock backend handle.
    pub fn new(key: &'static str, caps: CapabilitySet) -> Self {
        Self { key, caps }
    }
}

#[async_trait]
impl CameraBackend for MockBackendHandle {
    fn driver(&self) -> &'static str {
        self.key
    }

    fn capabilities(&self) -> CapabilitySet {
        self.caps
    }

    async fn probe(&self) -> Result<ProbeResult, CameraError> {
        Ok(ProbeResult {
            vendor: "MockVendor".to_string(),
            model: "MockModel".to_string(),
            firmware: Some("1.0.0".to_string()),
            ip: Some("127.0.0.1".to_string()),
            mac: None,
            ptz_supported: self.caps.contains(Capability::PtzMove),
            ai_supported: self.caps.contains(Capability::DetectMotion),
            stream_url: Some("rtsp://127.0.0.1:554/live".to_string()),
        })
    }

    async fn stream(
        &self,
        quality: cctv_core::StreamQuality,
    ) -> Result<cctv_core::StreamInfo, CameraError> {
        Ok(cctv_core::StreamInfo {
            uri: "rtsp://127.0.0.1:554/live".to_string(),
            profile: "profile_main".to_string(),
            quality,
            width: Some(1920),
            height: Some(1080),
            codec: Some("H264".to_string()),
        })
    }
}

/// A mock driver adapter satisfying the [`DriverAdapter`] contract.
#[derive(Debug, Clone)]
pub struct MockDriverAdapter {
    key: &'static str,
    caps: CapabilitySet,
}

impl MockDriverAdapter {
    /// Create a new mock driver adapter with given driver key.
    pub fn new(key: &'static str) -> Self {
        Self {
            key,
            caps: CapabilitySet::from_slice(&[
                Capability::StreamH264,
                Capability::PtzMove,
                Capability::PtzPresets,
                Capability::CameraContinuous,
            ]),
        }
    }

    /// Set capabilities for this mock adapter.
    pub fn with_capabilities(mut self, caps: CapabilitySet) -> Self {
        self.caps = caps;
        self
    }
}

impl Default for MockDriverAdapter {
    fn default() -> Self {
        Self::new("mock")
    }
}

#[async_trait]
impl DriverAdapter for MockDriverAdapter {
    fn driver_key(&self) -> &'static str {
        self.key
    }

    fn capabilities(&self) -> CapabilitySet {
        self.caps
    }

    async fn probe(&self, config: &CameraConfig) -> Result<ProbeResult, CameraError> {
        Ok(ProbeResult {
            vendor: "MockVendor".to_string(),
            model: "MockModel".to_string(),
            firmware: Some("1.0.0".to_string()),
            ip: Some(config.host.clone()),
            mac: None,
            ptz_supported: self.caps.contains(Capability::PtzMove),
            ai_supported: self.caps.contains(Capability::DetectMotion),
            stream_url: Some(format!("rtsp://{}:554/live", config.host)),
        })
    }

    async fn attach(&self, _config: &CameraConfig) -> Result<Box<dyn CameraBackend>, CameraError> {
        Ok(Box::new(MockBackendHandle::new(self.key, self.caps)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_adapter_probe_and_attach() {
        futures_executor::block_on(async {
            let adapter = MockDriverAdapter::new("mock");
            assert_eq!(adapter.driver_key(), "mock");
            let cfg = CameraConfig::new("c1", "mock", "10.0.0.1");
            let probe = adapter.probe(&cfg).await.expect("probe");
            assert_eq!(probe.vendor, "MockVendor");

            let backend = adapter.attach(&cfg).await.expect("attach");
            assert_eq!(backend.driver(), "mock");
            assert!(backend.capabilities().contains(Capability::StreamH264));
        });
    }
}
