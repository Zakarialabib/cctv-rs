//! ONVIF driver error mapping onto the workspace-wide [`CameraError`] taxonomy.

use cctv_core::CameraError;
use cctv_transport::http::HttpError;

use crate::onvif::error::OnvifError;

fn is_auth_fault(reason: &str) -> bool {
    let r = reason.to_ascii_lowercase();
    r.contains("notauthorized")
        || r.contains("not authorized")
        || r.contains("unauthorized")
        || r.contains("authentication")
}

/// Map an [`OnvifError`] to a [`CameraError`].
pub fn to_camera_error(error: impl core::borrow::Borrow<OnvifError>) -> CameraError {
    match error.borrow() {
        OnvifError::Http(HttpError::Connect(_)) => CameraError::Offline,
        OnvifError::Http(HttpError::Timeout) => CameraError::Network,
        OnvifError::Http(other) => CameraError::Other(other.to_string()),
        OnvifError::Status(401) => CameraError::AuthFailed,
        OnvifError::Status(403 | 405) => CameraError::Refused,
        OnvifError::Status(s) => CameraError::Other(format!("onvif device returned HTTP {s}")),
        OnvifError::Fault(r) if is_auth_fault(r) => CameraError::AuthFailed,
        OnvifError::Fault(r) => CameraError::Other(format!("onvif SOAP fault: {r}")),
        OnvifError::Parse(r) => CameraError::Other(format!("onvif reply unreadable: {r}")),
        OnvifError::MissingService("PTZ") => CameraError::PtzDisabled,
        OnvifError::MissingService(_) => CameraError::NotImplemented,
        OnvifError::NoProfile => CameraError::StreamFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_map_onto_the_camera_taxonomy() {
        assert_eq!(
            to_camera_error(OnvifError::Status(401)),
            CameraError::AuthFailed
        );
        assert_eq!(
            to_camera_error(OnvifError::Fault("ter:NotAuthorized".into())),
            CameraError::AuthFailed
        );
        assert_eq!(
            to_camera_error(OnvifError::Http(HttpError::Connect("x".into()))),
            CameraError::Offline
        );
        assert_eq!(
            to_camera_error(OnvifError::MissingService("PTZ")),
            CameraError::PtzDisabled
        );
        assert_eq!(
            to_camera_error(OnvifError::NoProfile),
            CameraError::StreamFailed
        );
    }
}
