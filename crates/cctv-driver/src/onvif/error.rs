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

impl From<OnvifError> for CameraError {
    fn from(e: OnvifError) -> Self {
        crate::map::onvif::to_camera_error(&e)
    }
}
