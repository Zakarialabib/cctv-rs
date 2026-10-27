//! Camera error taxonomy.
//!
//! The rule of the device model: *nothing panics*, and a capability a camera
//! does not have is a typed error, never `unimplemented!()` or `unwrap()`.
//!
//! Errors are intended to flow across a `Result<T, CameraError>` seam.  The
//! reconciler maps them to graded remediation.
//!
//! The `Display` text is the operator-facing sentence, and it is load-bearing:
//! `cctv-agent` puts it in the audit trail and the facade puts it in the
//! reconnection report.

use crate::prelude::*;

use thiserror::Error;

/// The typed failure taxonomy every camera driver returns.
///
/// A capability the device does not have is one of these variants rather than a
/// panic, so callers can map each failure to graded remediation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CameraError {
    /// The device refused the command (401/403/405).  Check auth state.
    #[error("camera refused the command")]
    Refused,
    /// The device is not reachable.  Reconnect attempts are warranted.
    #[error("camera is offline")]
    Offline,
    /// The stream dropped or the frame was invalid.
    #[error("camera stream failed")]
    StreamFailed,
    /// The camera cannot perform PTZ right now (hardware locked, firmware).
    #[error("camera PTZ is disabled")]
    PtzDisabled,
    /// No camera matched the requested identity.
    #[error("no camera matched the identity")]
    NoCamera,
    /// Access token missing, expired, or rejected.
    #[error("camera auth failed")]
    AuthFailed,
    /// The requested command is not implemented by this vendor.
    #[error("command not implemented by this vendor")]
    NotImplemented,
    /// Network-level failure (DNS, connect, timeout).
    #[error("network failure")]
    Network,
    /// Capture (still/image) failed.
    #[error("capture failed")]
    CaptureFailed,
    /// The device or subsystem is held by another process or OS shell (e.g. EBUSY, MF_E_ALREADY_INITIALIZED).
    #[error("camera device is busy")]
    DeviceBusy,
    /// Storage or media is writing or locked (e.g. PTP StoreNotReady).
    #[error("camera media is unavailable")]
    MediaUnavailable,
    /// Unknown — the driver surfaced something unclassified.
    #[error("{0}")]
    Other(String),
}

impl From<CameraError> for String {
    fn from(e: CameraError) -> Self {
        e.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_display_text_is_the_operator_sentence() {
        // These strings reach the audit trail and the reconnection report, so
        // they are asserted rather than left to drift.
        assert_eq!(CameraError::Offline.to_string(), "camera is offline");
        assert_eq!(CameraError::DeviceBusy.to_string(), "camera device is busy");
        assert_eq!(
            CameraError::MediaUnavailable.to_string(),
            "camera media is unavailable"
        );
        assert_eq!(
            CameraError::PtzDisabled.to_string(),
            "camera PTZ is disabled"
        );
        assert_eq!(
            CameraError::NotImplemented.to_string(),
            "command not implemented by this vendor"
        );
        assert_eq!(
            CameraError::Other("vendor said no".into()).to_string(),
            "vendor said no",
            "an unclassified error keeps the driver's own words"
        );
    }

    #[test]
    fn the_error_implements_the_standard_traits() {
        fn assert_error<E: core::error::Error + Clone + PartialEq + Eq>() {}
        assert_error::<CameraError>();

        let stringed: String = CameraError::AuthFailed.into();
        assert_eq!(stringed, "camera auth failed");
    }
}
