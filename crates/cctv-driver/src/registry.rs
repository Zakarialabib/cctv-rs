#[cfg(test)]
mod tests {
    use super::*;
    use cctv_core::{Capability, CapabilitySet, CameraError, StreamQuality};

    /// Minimal dummy adapter for registry round-trips.
    struct DummyAdapter {
        key: &'static str,
        name: &'static str,
    }

    impl DriverAdapter for DummyAdapter {
        fn driver_key(&self) -> &'static str {
            self.key
        }

        fn name(&self) -> &'static str {
            self.name
        }

        fn capabilities(&self) -> cctv_core::CapabilitySet {
            CapabilitySet::new(vec![Capability::StreamH264])
        }

        async fn probe(&self, _id: &str) -> Result<cctv_core::ProbeResult, CameraError> {
            Ok(cctv_core::ProbeResult {
                vendor: self.name.to_string(),
                model: "dummy".to_string(),
                firmware: "0.0.0".to_string(),
                ip: "127.0.0.1".to_string(),
                mac: None,
                ptz_supported: false,
                ai_supported: false,
                stream_url: None,
            })
        }

        async fn stream(
            &self,
            _id: &str,
            quality: StreamQuality,
        ) -> Result<StreamInfo, CameraError> {
            Ok(StreamInfo {
                url: format!("rtsp://127.0.0.1/{}/Streaming/Channels/101", quality.head),
                quality,
            })
        }

        async fn ptz(&self, _id: &str, _command: PtzCommand) -> Result<(), CameraError> {
            Err(CameraError::NotImplemented)
        }

        async fn presets(&self, _id: &str) -> Result<Vec<Preset>, CameraError> {
            Ok(vec![])
        }

        async fn save_preset(&self, _id: &str, _config: &str) -> Result<(), CameraError> {
            Err(CameraError::NotImplemented)
        }

        async fn delete_preset(&self, _id: &str) -> Result<(), CameraError> {
            Err(CameraError::NotImplemented)
        }

        async fn goto_preset(&self, _id: &str, _preset: &str) -> Result<(), CameraError> {
            Err(CameraError::NotImplemented)
        }
    }

    #[test]
    fn registry_roundtrip() {
        let adapter = DummyAdapter {
            key: "dummy",
            name: "Dummy",
        };
        let mut registry = InMemoryRegistry::new();
        assert_eq!(registry.keys().len(), 0);
        registry.register(&adapter);
        assert_eq!(registry.keys().len(), 1);
        assert!(registry.get("dummy").is_some());
        assert!(registry.get("missing").is_none());
        assert_eq!(registry.remove("dummy"), Some(adapter));
        assert!(registry.get("dummy").is_none());
    }
}
