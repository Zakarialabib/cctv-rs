//! UVC and PTP adapters and OS seam crate.
//!
//! Provides the Ports and Adapters implementation for UVC video streaming,
//! domain-driven backpressure via [`BudgetedFrameReader`], PTP "Capture & Fetch"
//! surfaces via [`PtpAdapter`], and HAL hot-plug event sources.

#![forbid(unsafe_code)]

pub mod backpressure;
pub mod hal;
pub mod ptp;
pub mod uvc;

pub use backpressure::{BudgetedFrameReader, FrameItem};
pub use hal::HalEventSource;
pub use ptp::{PtpAdapter, PtpStatus};
pub use uvc::{OsError, OsPlatform, UvcAdapter};
