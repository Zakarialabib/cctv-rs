//! UVC and PTP adapters and OS seam crate.
//!
//! Provides the Ports and Adapters implementation for UVC video streaming,
//! domain-driven backpressure via [`BudgetedFrameReader`], PTP "Capture & Fetch"
//! surfaces via [`PtpAdapter`], and HAL hot-plug event sources.

#![forbid(unsafe_code)]

pub mod backpressure;
#[cfg(feature = "ddc")]
pub mod ddc;
pub mod hal;
pub mod observe_bridge;
pub mod ptp;
pub mod uvc;

pub use backpressure::{BudgetedFrameReader, FrameItem};
#[cfg(feature = "ddc")]
pub use ddc::{
    build_get_vcp_packet, build_set_vcp_packet, compute_checksum, parse_get_vcp_reply,
    DdcController, DdcError, DisplayProfile, I2cBus, InputSource, LinuxI2cBus, MockI2cBus,
    PowerMode, VcpCode, DDC_HOST_ADDR, DDC_I2C_ADDR,
};
pub use hal::HalEventSource;
pub use observe_bridge::BroadcastObservationSource;
pub use ptp::{PtpAdapter, PtpStatus};
pub use uvc::{OsError, OsPlatform, UvcAdapter};
