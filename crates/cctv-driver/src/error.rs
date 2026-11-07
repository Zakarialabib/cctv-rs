//! Errors a vendor adapter introduces, mapped onto the workspace-wide
//! `cctv_core::CameraError` taxonomy so adapters cannot invent a second error
//! surface. A vendor whose error taxonomy diverges keeps its own error type
//! for internal diagnostics; the seam guarantees the facade always sees
//! `CameraError`.

use cctv_core::CameraError;

/// A vendor adapter's own error type (e.g. an HTTP or SOAP error).
#[derive(Debug, Clone, thiserror::Error)]
pub enum DriverError {
    /// The wire said what it said; the call reached the device.
    #[error("vendor error: {0}")]
    Wire(String),

    /// The adapter's configuration could not be understood.
    #[error("configuration error: {0}")]
    Config(String),

    /// The vendor service is not reachable.
    #[error("unreachable: {0}")]
    Unreachable(String),

    /// The vendor rejected the credentials or scope.
    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("not supported: {0}")]
    NotSupported(String),
}

impl From<DriverError> for CameraError {
    fn from(e: DriverError) -> Self {
        match e {
            DriverError::Wire(ref s) => CameraError::Other(s),
            DriverError::Config(ref s) => CameraError::Other(format!("config: {s}")),
            DriverError::Unreachable(ref s) => CameraError::Other(format!("unreachable: {s}")),
            DriverError::Unauthorized(ref s) => CameraError::Unauthorized(s),
            DriverError::NotSupported(ref s) => CameraError::PtzDisabled, // the seam has no vendor presets
        }
    }
}
