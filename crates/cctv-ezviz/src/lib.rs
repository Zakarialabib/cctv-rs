//! Ezviz Open Platform driver for cctv-rs.
//!
//! The one crate that speaks the Ezviz protocol.  It wraps the standalone
//! [`ezviz_openapi`] client and translates its responses into the `cctv-core`
//! device model, so vendor wire detail stops at this boundary and never reaches
//! the `cctv-rs` facade.
//!
//! - [`Driver`] — the account-level handle: `discover`, `alarms`, `capture`.
//! - [`EzvizCameraBackend`] — one camera / channel, as a
//!   [`cctv_core::CameraBackend`]; the subject of the conformance suite.
//! - [`EzvizCamera`] / [`Connectivity`] / [`Alarm`] — the device-model shapes
//!   the driver hands back.
//! - [`ptz`] — the pure translation of a [`cctv_core::PtzCommand`] into the
//!   platform's discrete direction + speed vocabulary.
//! - [`DriverError`] — the driver's failure type, and its mapping onto
//!   [`cctv_core::CameraError`].
//!
//! The vendor client is part of this crate's surface: a caller that needs an
//! endpoint the driver does not model reaches for [`EzvizClient`] directly.

#![forbid(unsafe_code)]

pub mod backend;
pub mod driver;
pub mod error;
pub mod ptz;
pub mod types;

pub use backend::EzvizCameraBackend;
pub use driver::Driver;
pub use error::DriverError;
pub use types::{Alarm, Connectivity, EzvizCamera};

pub use ezviz_openapi::EzvizClient;
