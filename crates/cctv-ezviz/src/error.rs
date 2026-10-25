//! The Ezviz driver's error type and its mapping onto [`CameraError`].
//!
//! The vendor client's `EzvizError` is flattened into the three shapes the
//! driver can act on, plus the one it decides by itself: an operation the Ezviz
//! protocol cannot express.

use cctv_core::CameraError;
use ezviz_openapi::EzvizError;

/// A failure inside the Ezviz driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverError {
    /// The platform rejected the call, preserving its own `code` and `msg`.
    Api {
        /// Platform status code from the JSON envelope.
        code: String,
        /// Platform message from the JSON envelope.
        msg: String,
    },
    /// The transport failed before a reply arrived (DNS, TLS, connect, timeout).
    Transport(String),
    /// A reply could not be decoded into the shape the client expects.
    Decode(String),
    /// The operation is not expressible in the Ezviz protocol.
    Unsupported(&'static str),
}

impl core::fmt::Display for DriverError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Api { code, msg } => write!(f, "ezviz platform error {code}: {msg}"),
            Self::Transport(e) => write!(f, "ezviz transport failed: {e}"),
            Self::Decode(e) => write!(f, "ezviz reply unreadable: {e}"),
            Self::Unsupported(what) => write!(f, "ezviz cannot {what}"),
        }
    }
}

impl std::error::Error for DriverError {}

/// Crate-local result alias.
pub type Result<T> = core::result::Result<T, DriverError>;

impl From<EzvizError> for DriverError {
    fn from(error: EzvizError) -> Self {
        match error {
            EzvizError::Api { code, msg } => Self::Api { code, msg },
            EzvizError::Http(e) => Self::Transport(e.to_string()),
            EzvizError::Json(e) => Self::Decode(e.to_string()),
            // `Io` (and `Sdk`, with the `pc-sdk` feature) are local failures.
            other => Self::Transport(other.to_string()),
        }
    }
}

impl From<DriverError> for CameraError {
    fn from(error: DriverError) -> Self {
        match error {
            // The platform's codes are not a stable contract — the vendor's own
            // documentation says the numeric ranges differ between regions and
            // firmware generations, and the client deliberately does not
            // reinterpret them.  Only `10002` is acted on: it is "token expired"
            // even after the client's single refresh-and-replay, so it is an
            // auth failure by definition.  Everything else keeps its code and
            // message in `Other`.
            DriverError::Api { code, msg } => {
                if code == "10002" {
                    CameraError::AuthFailed
                } else {
                    CameraError::Other(format!("ezviz platform error {code}: {msg}"))
                }
            }
            DriverError::Transport(_) => CameraError::Network,
            DriverError::Decode(e) => CameraError::Other(format!("ezviz reply unreadable: {e}")),
            DriverError::Unsupported(_) => CameraError::NotImplemented,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_errors_map_onto_the_camera_taxonomy() {
        assert_eq!(
            CameraError::from(DriverError::Api {
                code: "10002".into(),
                msg: "token expired".into()
            }),
            CameraError::AuthFailed
        );
        assert_eq!(
            CameraError::from(DriverError::Transport("dns".into())),
            CameraError::Network
        );
        assert_eq!(
            CameraError::from(DriverError::Unsupported("absolute move")),
            CameraError::NotImplemented
        );
        // Any other platform code is preserved verbatim, never reinterpreted.
        let other = CameraError::from(DriverError::Api {
            code: "20002".into(),
            msg: "device offline".into(),
        });
        assert_eq!(
            other,
            CameraError::Other("ezviz platform error 20002: device offline".into())
        );
    }

    #[test]
    fn the_vendor_error_is_flattened_without_losing_the_code() {
        let mapped = DriverError::from(EzvizError::Api {
            code: "60001".into(),
            msg: "rate limited".into(),
        });
        assert_eq!(
            mapped,
            DriverError::Api {
                code: "60001".into(),
                msg: "rate limited".into()
            }
        );
        assert!(mapped.to_string().contains("60001"));
    }
}
