//! UVC and PTP adapters and OS seam.

pub mod backpressure;
#[cfg(feature = "ddc")]
pub mod ddc;
pub mod hal;
#[cfg(feature = "uvc-ptp")]
pub mod ptp;
#[allow(clippy::module_inception)]
pub mod uvc;

pub use backpressure::{BudgetedFrameReader, FrameItem};
#[cfg(feature = "ddc")]
pub use ddc::{
    build_get_vcp_packet, build_set_vcp_packet, compute_checksum, parse_get_vcp_reply,
    DdcController, DdcError, DisplayProfile, I2cBus, InputSource, LinuxI2cBus, MockI2cBus,
    PowerMode, VcpCode, DDC_HOST_ADDR, DDC_I2C_ADDR,
};
pub use hal::HalEventSource;
#[cfg(feature = "uvc-ptp")]
pub use ptp::{PtpAdapter, PtpStatus};
pub use uvc::{OsError, OsPlatform, UvcAdapter};

use async_trait::async_trait;
use cctv_core::{CameraBackend, CameraConfig, CameraError, CapabilitySet, ProbeResult};

use crate::DriverAdapter;

/// Driver adapter for standard UVC USB/host cameras.
#[derive(Debug, Default, Clone)]
pub struct UvcDriverAdapter;

impl UvcDriverAdapter {
    /// Create a new UVC driver adapter.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DriverAdapter for UvcDriverAdapter {
    fn driver_key(&self) -> &'static str {
        "uvc"
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::from_slice(&[
            cctv_core::Capability::StreamH264,
            cctv_core::Capability::CameraContinuous,
            cctv_core::Capability::AudioIn,
        ])
    }

    async fn probe(&self, config: &CameraConfig) -> Result<ProbeResult, CameraError> {
        let backend = self.attach(config).await?;
        backend.probe().await
    }

    async fn attach(&self, config: &CameraConfig) -> Result<Box<dyn CameraBackend>, CameraError> {
        let device_path = if config.host.is_empty() {
            "/dev/video0".to_string()
        } else {
            config.host.clone()
        };
        #[cfg(target_os = "windows")]
        let platform = OsPlatform::WindowsMediaFoundation;
        #[cfg(target_os = "macos")]
        let platform = OsPlatform::MacOsAvFoundation;
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let platform = OsPlatform::LinuxV4l2;

        Ok(Box::new(UvcAdapter::new(&config.id, device_path, platform)))
    }
}

#[cfg(feature = "uvc-ptp")]
/// Driver adapter for PTP "Capture & Fetch" cameras.
#[derive(Debug, Default, Clone)]
pub struct PtpDriverAdapter;

#[cfg(feature = "uvc-ptp")]
impl PtpDriverAdapter {
    /// Create a new PTP driver adapter.
    pub fn new() -> Self {
        Self
    }
}

#[cfg(feature = "uvc-ptp")]
#[async_trait]
impl DriverAdapter for PtpDriverAdapter {
    fn driver_key(&self) -> &'static str {
        "ptp"
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::from_slice(&[cctv_core::Capability::Camera4k])
    }

    async fn probe(&self, config: &CameraConfig) -> Result<ProbeResult, CameraError> {
        let backend = self.attach(config).await?;
        backend.probe().await
    }

    async fn attach(&self, config: &CameraConfig) -> Result<Box<dyn CameraBackend>, CameraError> {
        let serial = if config.host.is_empty() {
            "PTP-SN-001".to_string()
        } else {
            config.host.clone()
        };
        Ok(Box::new(PtpAdapter::new(&config.id, serial)))
    }
}
