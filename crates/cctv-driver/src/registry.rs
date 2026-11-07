//! Registry of [`DriverAdapter`] factories.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use cctv_core::{CameraBackend, CameraConfig, CameraError};

use crate::DriverAdapter;

/// Trait for driver registries mapping driver keys to [`DriverAdapter`] instances.
#[async_trait]
pub trait DriverRegistry: Send + Sync {
    /// Register an adapter under its driver key.
    fn register(&mut self, adapter: Arc<dyn DriverAdapter>);

    /// List all registered driver keys.
    fn keys(&self) -> Vec<String>;

    /// Lookup a driver adapter by its key.
    fn get(&self, key: &str) -> Option<Arc<dyn DriverAdapter>>;

    /// Remove a driver adapter by key.
    fn remove(&mut self, key: &str) -> Option<Arc<dyn DriverAdapter>>;

    /// Route `attach` through the registry to build a [`CameraBackend`] handle.
    async fn attach(&self, config: &CameraConfig) -> Result<Box<dyn CameraBackend>, CameraError> {
        let adapter = self.get(&config.driver).ok_or_else(|| {
            CameraError::Other(format!("no driver registered for {:?}", config.driver))
        })?;
        adapter.attach(config).await
    }
}

/// An in-memory [`DriverRegistry`] backed by a `BTreeMap`.
#[derive(Default, Clone)]
pub struct InMemoryRegistry {
    adapters: BTreeMap<String, Arc<dyn DriverAdapter>>,
}

impl InMemoryRegistry {
    /// Create a new empty in-memory driver registry.
    pub fn new() -> Self {
        Self {
            adapters: BTreeMap::new(),
        }
    }
}

#[async_trait]
impl DriverRegistry for InMemoryRegistry {
    fn register(&mut self, adapter: Arc<dyn DriverAdapter>) {
        self.adapters
            .insert(adapter.driver_key().to_string(), adapter);
    }

    fn keys(&self) -> Vec<String> {
        self.adapters.keys().cloned().collect()
    }

    fn get(&self, key: &str) -> Option<Arc<dyn DriverAdapter>> {
        self.adapters.get(key).cloned()
    }

    fn remove(&mut self, key: &str) -> Option<Arc<dyn DriverAdapter>> {
        self.adapters.remove(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cctv_core::{Capability, CapabilitySet, ProbeResult};

    struct DummyBackend {
        key: &'static str,
    }

    #[async_trait]
    impl CameraBackend for DummyBackend {
        fn driver(&self) -> &'static str {
            self.key
        }

        async fn probe(&self) -> Result<ProbeResult, CameraError> {
            Ok(ProbeResult {
                vendor: "Dummy".to_string(),
                model: "dummy".to_string(),
                firmware: Some("0.0.0".to_string()),
                ip: Some("127.0.0.1".to_string()),
                mac: None,
                ptz_supported: false,
                ai_supported: false,
                stream_url: None,
            })
        }

        async fn stream(
            &self,
            _quality: cctv_core::StreamQuality,
        ) -> Result<cctv_core::StreamInfo, CameraError> {
            Err(CameraError::NotImplemented)
        }
    }

    /// Minimal dummy adapter for registry round-trips.
    struct DummyAdapter {
        key: &'static str,
    }

    #[async_trait]
    impl DriverAdapter for DummyAdapter {
        fn driver_key(&self) -> &'static str {
            self.key
        }

        fn capabilities(&self) -> CapabilitySet {
            CapabilitySet::from_slice(&[Capability::StreamH264])
        }

        async fn probe(&self, _config: &CameraConfig) -> Result<ProbeResult, CameraError> {
            Ok(ProbeResult {
                vendor: "Dummy".to_string(),
                model: "dummy".to_string(),
                firmware: Some("0.0.0".to_string()),
                ip: Some("127.0.0.1".to_string()),
                mac: None,
                ptz_supported: false,
                ai_supported: false,
                stream_url: None,
            })
        }

        async fn attach(
            &self,
            _config: &CameraConfig,
        ) -> Result<Box<dyn CameraBackend>, CameraError> {
            Ok(Box::new(DummyBackend { key: self.key }))
        }
    }

    #[test]
    fn registry_roundtrip() {
        futures_executor::block_on(async {
            let adapter = Arc::new(DummyAdapter { key: "dummy" });
            let mut registry = InMemoryRegistry::new();
            assert_eq!(registry.keys().len(), 0);
            registry.register(adapter.clone());
            assert_eq!(registry.keys().len(), 1);
            assert!(registry.get("dummy").is_some());
            assert!(registry.get("missing").is_none());

            let config = CameraConfig::new("c1", "dummy", "127.0.0.1");
            let backend = registry.attach(&config).await.expect("attached");
            assert_eq!(backend.driver(), "dummy");

            assert!(registry.remove("dummy").is_some());
            assert!(registry.get("dummy").is_none());
        });
    }
}
