//! Ezviz Open Platform driver — camera API surface.
//!
//! Where the Ezviz protocol meets the cctv device model. Every public method here
//! is a *driver* operation; the user-facing surface is `cctv-core`'s traits.

use crate::client::EzvizClient;
use crate::types::EzvizCamera;

/// The Ezviz driver.
pub struct Driver {
    client: EzvizClient,
}

impl Driver {
    /// Build from a configured Ezviz account token.
    pub fn new(client: EzvizClient) -> Self {
        Self { client }
    }

    /// The underlying Ezviz Open API client.
    pub fn client(&self) -> &EzvizClient {
        &self.client
    }

    /// List online cameras.
    pub fn discover(&self) -> Result<Vec<EzvizCamera>, String> {
        // Delegate to ezviz_openapi, then translate into the cctv device model.
        Ok(vec![]) // stub
    }
}
