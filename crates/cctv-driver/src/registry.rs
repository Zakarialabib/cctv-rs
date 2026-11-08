//! Registry of [`DriverAdapter`] factories.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use cctv_core::{CameraBackend, CameraConfig, CameraError, ProbeResult};

use crate::fingerprint::priority;
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

    /// List all registered adapters.
    fn all(&self) -> Vec<Arc<dyn DriverAdapter>>;

    /// Resolve a driver adapter for a camera configuration using the five-step ladder:
    ///
    /// 1. **Explicit Key Match**: If `config.driver` names an active registered driver, use it.
    /// 2. **Specific Vendor Fingerprint**: Highest-priority vendor match (`priority < OEM`).
    /// 3. **OEM / White-label Platform**: Matching OEM platform rules (`priority < FALLBACK`).
    /// 4. **ONVIF Fallback**: Standard ONVIF profile fallback (`priority <= FALLBACK` or `"onvif"`).
    /// 5. **RTSP Template Fallback**: Fallback adapter exposing usable RTSP templates.
    fn resolve(
        &self,
        config: &CameraConfig,
        candidate: Option<&ProbeResult>,
    ) -> Option<Arc<dyn DriverAdapter>> {
        // Step 1: Explicit driver match if non-empty and not "auto"
        let explicit = config.driver.trim();
        if !explicit.is_empty() && !explicit.eq_ignore_ascii_case("auto") {
            if let Some(adapter) = self.get(explicit) {
                return Some(adapter);
            }
        }

        // Collect and sort adapters by priority ascending (lower value = higher priority)
        let mut adapters = self.all();
        adapters.sort_by_key(|a| a.priority());

        if let Some(probe) = candidate {
            // Step 2: Specific vendor fingerprint match (priority < OEM)
            for adapter in adapters.iter().filter(|a| a.priority() < priority::OEM) {
                if adapter.fingerprint().matches(probe) {
                    return Some(adapter.clone());
                }
            }

            // Step 3: OEM / white-label platform match (OEM <= priority < FALLBACK)
            for adapter in adapters
                .iter()
                .filter(|a| a.priority() >= priority::OEM && a.priority() < priority::FALLBACK)
            {
                if adapter.fingerprint().matches(probe) {
                    return Some(adapter.clone());
                }
            }

            // Step 4: ONVIF fallback match if probe indicates ONVIF or service_url / scope present
            if probe.stream_url.is_some()
                || config.service_url.is_some()
                || probe.vendor.to_lowercase().contains("onvif")
            {
                if let Some(onvif) = adapters
                    .iter()
                    .find(|a| a.driver_key() == "onvif" || a.priority() == priority::FALLBACK)
                {
                    return Some(onvif.clone());
                }
            }
        }

        // Still Step 4: Default ONVIF fallback if available
        if let Some(onvif) = self.get("onvif") {
            return Some(onvif);
        }

        // Step 5: RTSP template generic fallback (first adapter with rtsp_templates or highest priority)
        for adapter in &adapters {
            if !adapter.rtsp_templates().is_empty() {
                return Some(adapter.clone());
            }
        }

        // Final fallback: first available adapter
        adapters.into_iter().next()
    }

    /// Route `attach` through the registry to build a [`CameraBackend`] handle.
    async fn attach(&self, config: &CameraConfig) -> Result<Box<dyn CameraBackend>, CameraError> {
        let adapter = self.resolve(config, None).ok_or_else(|| {
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

    fn all(&self) -> Vec<Arc<dyn DriverAdapter>> {
        self.adapters.values().cloned().collect()
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
