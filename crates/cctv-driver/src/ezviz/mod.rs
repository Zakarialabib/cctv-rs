//! Ezviz Open Platform driver integration for cctv-rs.

pub mod backend;
pub mod driver;
pub mod error;
pub mod ptz;
pub mod types;

use async_trait::async_trait;
use cctv_core::{CameraBackend, CameraConfig, CameraError, CapabilitySet, ProbeResult};
pub use ezviz_openapi::EzvizClient;

pub use backend::EzvizCameraBackend;
pub use driver::Driver as EzvizDriver;
pub use error::DriverError as EzvizError;
pub use types::{Alarm, Connectivity, EzvizCamera};

use crate::DriverAdapter;

/// Driver adapter for Ezviz Open Platform cloud cameras.
#[derive(Debug, Default, Clone)]
pub struct EzvizDriverAdapter;

impl EzvizDriverAdapter {
    /// Create a new Ezviz driver adapter.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DriverAdapter for EzvizDriverAdapter {
    fn driver_key(&self) -> &'static str {
        "ezviz"
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::from_slice(&[
            cctv_core::Capability::StreamH264,
            cctv_core::Capability::StreamH265,
            cctv_core::Capability::PtzMove,
            cctv_core::Capability::CameraContinuous,
            cctv_core::Capability::DetectMotion,
            cctv_core::Capability::LightTorch,
        ])
    }

    fn priority(&self) -> u16 {
        crate::fingerprint::priority::SPECIFIC
    }

    fn fingerprint(&self) -> crate::fingerprint::FingerprintRule {
        crate::fingerprint::FingerprintRule::new()
            .with_manufacturer("ezviz")
            .with_model("cs-")
    }

    async fn probe(&self, config: &CameraConfig) -> Result<ProbeResult, CameraError> {
        let backend = self.attach(config).await?;
        backend.probe().await
    }

    async fn attach(&self, config: &CameraConfig) -> Result<Box<dyn CameraBackend>, CameraError> {
        let serial = config.host.trim();
        if serial.is_empty() {
            return Err(CameraError::NoCamera);
        }
        let channel = i32::try_from(config.channel_or_default()).unwrap_or(1);
        let client = match (config.username.as_deref(), config.password.as_deref()) {
            (Some(app_key), Some(app_secret)) => EzvizClient::new(app_key, app_secret),
            (None, Some(token)) => EzvizClient::with_token(token, config.service_url.clone()),
            _ => return Err(CameraError::AuthFailed),
        };
        Ok(Box::new(EzvizCameraBackend::new(client, serial, channel)))
    }
}
