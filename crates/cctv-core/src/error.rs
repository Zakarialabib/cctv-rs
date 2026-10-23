//! Camera error taxonomy.
//!
//! The rule of the device model: *nothing panics*, and a capability a camera
//! does not have is a typed error, never `unimplemented!()` or `unwrap()`.
//!
//! Errors are intended to flow across a `Result<T, CameraError>` seam.  The
//! reconciler maps them to graded remediation.

use core::fmt;

/// The typed failure taxonomy every camera driver returns.
///
/// A capability the device does not have is one of these variants rather than a
/// panic, so callers can map each failure to graded remediation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CameraError {
    /// The device refused the command (401/403/405).  Check auth state.
    Refused,
    /// The device is not reachable.  Reconnect attempts are warranted.
    Offline,
    /// The stream dropped or the frame was invalid.
    StreamFailed,
    /// The camera cannot perform PTZ right now (hardware locked, firmware).
    PtzDisabled,
    /// No camera matched the requested identity.
    NoCamera,
    /// Access token missing, expired, or rejected.
    AuthFailed,
    /// The requested command is not implemented by this vendor.
    NotImplemented,
    /// Network-level failure (DNS, connect, timeout).
    Network,
    /// Capture (still/image) failed.
    CaptureFailed,
    /// Unknown — the driver surfaced something unclassified.
    Other(String),
}

impl fmt::Display for CameraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let msg = match self {
            Self::Refused => "camera refused the command",
            Self::Offline => "camera is offline",
            Self::StreamFailed => "camera stream failed",
            Self::PtzDisabled => "camera PTZ is disabled",
            Self::NoCamera => "no camera matched the identity",
            Self::AuthFailed => "camera auth failed",
            Self::NotImplemented => "command not implemented by this vendor",
            Self::Network => "network failure",
            Self::CaptureFailed => "capture failed",
            Self::Other(s) => s,
        };
        write!(f, "{msg}")
    }
}

impl std::error::Error for CameraError {}

impl From<CameraError> for String {
    fn from(e: CameraError) -> Self {
        e.to_string()
    }
}
