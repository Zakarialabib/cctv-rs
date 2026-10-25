//! The driver registry: a key in a connection record, resolved to a driver.
//!
//! [`CameraConfig::driver`] is a *key* (data), not a type, so no vendor name is
//! part of the facade's API.  The registry maps that key to a factory; the
//! factories below are the ones this workspace ships.

use std::sync::Arc;

use cctv_core::{CameraBackend, CameraConfig, CameraError};
use cctv_ezviz::{EzvizCameraBackend, EzvizClient};
use cctv_onvif::{Credentials, OnvifCameraBackend, OnvifSession};
use cctv_transport::http::HttpConfig;

/// Builds a [`CameraBackend`] for one camera from its connection record.
///
/// A factory is the only place a vendor crate is named, and it is reached
/// through data (`CameraConfig::driver`), never through the facade's API.
pub type DriverFactory =
    Arc<dyn Fn(&CameraConfig) -> Result<Arc<dyn CameraBackend>, CameraError> + Send + Sync>;

/// The key the built-in Ezviz factory is registered under.
pub const EZVIZ: &str = "ezviz";

/// The factory for [`EZVIZ`].
///
/// An Ezviz camera is a cloud device with no host of its own, so the
/// vendor-neutral connection record carries:
///
/// - `host` — the **device serial** (the platform addresses a camera by serial,
///   so this is the camera's address for this driver),
/// - `channel` — the channel, defaulting to 1,
/// - `service_url` — the region base URL, when a token was issued for one,
/// - `username` + `password` — the **app key and secret**, in which case the
///   client mints and caches the token itself; or `password` alone, as an access
///   token the application already holds.
pub fn ezviz(config: &CameraConfig) -> Result<Arc<dyn CameraBackend>, CameraError> {
    let serial = config.host.trim();
    if serial.is_empty() {
        return Err(CameraError::NoCamera);
    }
    let channel = i32::try_from(config.channel_or_default()).unwrap_or(1);
    let client = match (config.username.as_deref(), config.password.as_deref()) {
        (Some(app_key), Some(app_secret)) => EzvizClient::new(app_key, app_secret),
        (None, Some(token)) => EzvizClient::with_token(token, config.service_url.clone()),
        _ => return Err(CameraError::AuthFailed),
    };
    Ok(Arc::new(EzvizCameraBackend::new(client, serial, channel)))
}

/// The key the built-in ONVIF factory is registered under.
pub const ONVIF: &str = "onvif";

/// The factory for [`ONVIF`].
///
/// An ONVIF camera is addressed by its **device-service URL**, which the
/// connection record carries in `service_url` — the `XAddr` discovery reports.
/// When it is absent, one is built from `host` / `port` / `https`, because every
/// ONVIF device answers on `/onvif/device_service`.
///
/// `username` / `password` become the WS-Security `UsernameToken`, not HTTP
/// credentials: ONVIF authenticates at the message level.
pub fn onvif(config: &CameraConfig) -> Result<Arc<dyn CameraBackend>, CameraError> {
    let device_url = match config.service_url.as_deref() {
        Some(url) if !url.trim().is_empty() => url.to_string(),
        _ => format!("{}/onvif/device_service", config.base_url(80)),
    };
    let session = OnvifSession::new(
        device_url,
        Credentials {
            username: config.username.clone().unwrap_or_default(),
            password: config.password.clone().unwrap_or_default(),
        },
        HttpConfig {
            accept_invalid_certs: config.accept_invalid_certs,
            ..HttpConfig::default()
        },
    )?;
    Ok(Arc::new(OnvifCameraBackend::new(session)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cctv_core::CameraType;

    fn config() -> CameraConfig {
        let mut config = CameraConfig::new("front-door", EZVIZ, "CAM-SERIAL-1");
        config.ty = CameraType::Consumer;
        config
    }

    #[test]
    fn the_onvif_factory_builds_from_a_device_service_url() {
        let mut record = CameraConfig::new("lobby", ONVIF, "10.0.0.9");
        record.service_url = Some("http://10.0.0.9/onvif/device_service".into());
        record = record.with_credentials("admin", "hunter2");
        assert_eq!(onvif(&record).expect("a backend").driver(), "onvif");
    }

    #[test]
    fn the_onvif_factory_falls_back_to_the_standard_path() {
        // No `service_url`: every ONVIF device answers on this path.
        let record = CameraConfig::new("lobby", ONVIF, "10.0.0.9").with_credentials("a", "b");
        assert_eq!(onvif(&record).expect("a backend").driver(), "onvif");
    }

    #[test]
    fn the_ezviz_factory_builds_from_an_app_key_and_secret() {
        let config = config().with_credentials("app-key", "app-secret");
        let backend = ezviz(&config).expect("a backend");
        assert_eq!(backend.driver(), "ezviz");
    }

    #[test]
    fn the_ezviz_factory_accepts_a_held_token_and_a_region() {
        let mut config = config();
        config.password = Some("token".into());
        config.service_url = Some("https://iusopen.ezvizlife.com".into());
        assert_eq!(ezviz(&config).expect("a backend").driver(), "ezviz");
    }

    #[test]
    fn a_record_without_an_address_or_a_secret_is_refused() {
        // No serial: there is nothing to address.
        let mut no_host = CameraConfig::new("x", EZVIZ, "");
        no_host = no_host.with_credentials("k", "s");
        assert!(matches!(ezviz(&no_host), Err(CameraError::NoCamera)));

        // A serial but no credentials at all.
        assert!(matches!(ezviz(&config()), Err(CameraError::AuthFailed)));
    }
}
