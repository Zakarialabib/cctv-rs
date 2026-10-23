//! The ONVIF driver's error type and its mapping onto [`CameraError`].

use cctv_core::CameraError;
use cctv_transport::http::HttpError;

/// A failure inside the ONVIF driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnvifError {
    /// The HTTP exchange failed.
    Http(HttpError),
    /// The device answered with a non-success status and no SOAP fault.
    Status(u16),
    /// The device answered with a SOAP fault.
    Fault(String),
    /// The reply could not be parsed.
    Parse(String),
    /// The device does not advertise the service the call needs.
    MissingService(&'static str),
    /// The device published no media profile.
    NoProfile,
}

impl core::fmt::Display for OnvifError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "onvif http: {e}"),
            Self::Status(s) => write!(f, "onvif device returned HTTP {s}"),
            Self::Fault(r) => write!(f, "onvif SOAP fault: {r}"),
            Self::Parse(r) => write!(f, "onvif reply unreadable: {r}"),
            Self::MissingService(s) => write!(f, "onvif device has no {s} service"),
            Self::NoProfile => write!(f, "onvif device published no media profile"),
        }
    }
}

impl std::error::Error for OnvifError {}

/// Crate-local result alias.
pub type Result<T> = core::result::Result<T, OnvifError>;

impl From<HttpError> for OnvifError {
    fn from(e: HttpError) -> Self {
        Self::Http(e)
    }
}

fn is_auth_fault(reason: &str) -> bool {
    let r = reason.to_ascii_lowercase();
    r.contains("notauthorized") || r.contains("not authorized") || r.contains("unauthorized") || r.contains("authentication")
}

impl From<OnvifError> for CameraError {
    fn from(e: OnvifError) -> Self {
        match e {
            OnvifError::Http(HttpError::Connect(_)) => CameraError::Offline,
            OnvifError::Http(HttpError::Timeout) => CameraError::Network,
            OnvifError::Http(other) => CameraError::Other(other.to_string()),
            OnvifError::Status(401) => CameraError::AuthFailed,
            OnvifError::Status(403 | 405) => CameraError::Refused,
            OnvifError::Status(s) => CameraError::Other(format!("onvif device returned HTTP {s}")),
            OnvifError::Fault(r) if is_auth_fault(&r) => CameraError::AuthFailed,
            OnvifError::Fault(r) => CameraError::Other(format!("onvif SOAP fault: {r}")),
            OnvifError::Parse(r) => CameraError::Other(format!("onvif reply unreadable: {r}")),
            OnvifError::MissingService("PTZ") => CameraError::PtzDisabled,
            OnvifError::MissingService(_) => CameraError::NotImplemented,
            OnvifError::NoProfile => CameraError::StreamFailed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_map_onto_the_camera_taxonomy() {
        assert_eq!(CameraError::from(OnvifError::Status(401)), CameraError::AuthFailed);
        assert_eq!(CameraError::from(OnvifError::Fault("ter:NotAuthorized".into())), CameraError::AuthFailed);
        assert_eq!(CameraError::from(OnvifError::Http(HttpError::Connect("x".into()))), CameraError::Offline);
        assert_eq!(CameraError::from(OnvifError::MissingService("PTZ")), CameraError::PtzDisabled);
        assert_eq!(CameraError::from(OnvifError::NoProfile), CameraError::StreamFailed);
    }
}
