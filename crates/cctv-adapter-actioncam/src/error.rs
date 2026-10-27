//! Error types for ActionCam adapter operations.

use thiserror::Error;

/// Errors during action camera discovery or status inspection.
#[derive(Debug, Error)]
pub enum ActionCamError {
    /// An HTTP network error occurred.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// A JSON serialization or deserialization error occurred.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// The discovered device TXT record is not an Insta360 camera.
    #[error("Device is not an Insta360 camera: {0}")]
    NotAnInsta360(String),

    /// Missing or invalid host address.
    #[error("Invalid address: {0}")]
    InvalidAddress(String),
}
