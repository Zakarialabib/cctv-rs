//! Read-only camera status inspection client.

use serde::{Deserialize, Serialize};

use crate::error::ActionCamError;

/// Action Camera status snapshot parsed from `/status` JSON endpoint.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionCamStatus {
    /// Battery charge percentage (0..=100).
    pub battery_level: u8,
    /// Current recording state (e.g. "idle", "recording").
    pub recording_state: String,
    /// Storage space available in bytes.
    pub storage_remaining_bytes: u64,
    /// Internal core temperature in degrees Celsius, if reported.
    pub temperature_celsius: Option<f32>,
    /// Active capture mode (e.g. "video_360", "photo_360").
    pub capture_mode: String,
    /// Camera firmware version.
    pub firmware_version: Option<String>,
}

/// Passive ActionCam client that issues only read-only HTTP GET status requests.
pub struct ActionCamClient {
    http_client: reqwest::Client,
    base_url: String,
}

impl ActionCamClient {
    /// Construct a client pointed at the action camera base URL (e.g. `http://192.168.1.150:8080`).
    pub fn new(base_url: &str) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Fetch camera status JSON endpoint passively without sending any control/mutation bytes.
    pub async fn fetch_status(&self) -> Result<ActionCamStatus, ActionCamError> {
        let url = format!("{}/status", self.base_url);
        // Strictly passive: HTTP GET request only
        let response = self.http_client.get(&url).send().await?;
        let status = response
            .error_for_status()?
            .json::<ActionCamStatus>()
            .await?;
        Ok(status)
    }
}
