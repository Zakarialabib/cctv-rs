//! Driver surface for any vendor adapter that plugs into the `cctv-driver`
//! registry.
//!
//! §2.3: The DriverAdapter / CameraBackend split separates driver factories
//! (`DriverAdapter`) from live connected camera handles (`CameraBackend`).

#[cfg(feature = "discovery")]
pub mod discovery;
pub mod error;
pub mod map;
#[cfg(feature = "onvif")]
pub mod onvif;
pub mod registry;

#[cfg(feature = "discovery")]
pub use discovery::{Datagram, Discoverable, DiscoveredCamera, Probe};

#[cfg(feature = "driver-mock")]
pub mod mock;

#[cfg(feature = "ezviz")]
pub mod ezviz;

#[cfg(feature = "uvc")]
pub mod uvc;

use async_trait::async_trait;
use cctv_core::{CameraBackend, CameraConfig, CameraError, CapabilitySet, ProbeResult};

pub use error::DriverError;
pub use registry::{DriverRegistry, InMemoryRegistry};

#[cfg(feature = "driver-mock")]
pub use mock::MockDriverAdapter;

#[cfg(feature = "onvif")]
pub use onvif::{OnvifCameraBackend, OnvifDiscovery, OnvifDriverAdapter, OnvifError, OnvifSession};

#[cfg(feature = "ezviz")]
pub use ezviz::{EzvizCameraBackend, EzvizDriver, EzvizDriverAdapter, EzvizError};

#[cfg(feature = "uvc")]
pub use uvc::{
    BudgetedFrameReader, FrameItem, HalEventSource, OsError, OsPlatform, UvcAdapter,
    UvcDriverAdapter,
};

#[cfg(feature = "uvc-ptp")]
pub use uvc::{PtpAdapter, PtpDriverAdapter, PtpStatus};

#[cfg(all(feature = "uvc", feature = "ddc"))]
pub use uvc::{
    build_get_vcp_packet, build_set_vcp_packet, compute_checksum, parse_get_vcp_reply,
    DdcController, DdcError, DisplayProfile, I2cBus, InputSource, LinuxI2cBus, MockI2cBus,
    PowerMode, VcpCode, DDC_HOST_ADDR, DDC_I2C_ADDR,
};

/// A vendor/driver factory that can probe and attach to cameras of a specific kind.
///
/// Implements §2.3 factory responsibilities:
/// - `driver_key`: stable driver key (e.g., `"onvif"`, `"ezviz"`, `"mock"`)
/// - `capabilities`: capabilities advertised by this driver family
/// - `probe`: pure derivation or query of probe report
/// - `attach`: instantiate a [`CameraBackend`] handle from a connection record.
#[async_trait]
pub trait DriverAdapter: Send + Sync {
    /// Stable registry key, e.g. `"hikvision"`, `"ezviz"`, `"mock"`.
    fn driver_key(&self) -> &'static str;

    /// Human-readable name of the vendor/driver.
    fn name(&self) -> &'static str {
        self.driver_key()
    }

    /// Capabilities this adapter advertises (stream, ptz, presets, …).
    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::EMPTY
    }

    /// Build a probe report from the adapter's configuration or connection record.
    async fn probe(&self, config: &CameraConfig) -> Result<ProbeResult, CameraError>;

    /// Attach to a camera and construct a connected handle implementing [`CameraBackend`].
    async fn attach(&self, config: &CameraConfig) -> Result<Box<dyn CameraBackend>, CameraError>;
}
