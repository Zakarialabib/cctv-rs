//! ONVIF driver for cctv-rs — protocol only.
//!
//! SOAP 1.2 request builders and reply parsers for the ONVIF **Device**,
//! **Media** and **PTZ** services, the WS-Security `UsernameToken` header, and
//! the session that drives them over `cctv-transport`'s shared HTTP client.  The
//! ONVIF vocabulary — profiles, tokens, `XAddr` — stops at this crate's boundary:
//! everything the rest of the workspace sees is a `cctv-core` type.
//!
//! - [`soap`] — envelopes, the `#PasswordDigest` header, ISO 8601 timestamps.
//! - [`device`] — `GetDeviceInformation` and `GetCapabilities`.
//! - [`media`] — profiles, stream / snapshot URIs, the encoder write path.
//! - [`ptz`] — moves, stop and presets.
//! - [`session`] — one device on the wire: the builders above, POSTed.
//! - [`discovery`] — WS-Discovery sweeps delegated to `cctv-driver::discovery`,
//!   filtered to devices with ONVIF evidence and an addressable `XAddr`.
//! - [`backend`] — one camera as a [`cctv_core::CameraBackend`], which is what
//!   the facade and the conformance suite drive.
//! - [`error`] — [`OnvifError`] and its mapping onto [`cctv_core::CameraError`].
//!
//! Builders return SOAP **body** fragments; [`soap::envelope`] wraps them.
//! Parsers are namespace-prefix-agnostic, so `tds:`, `SOAP-ENV:` and unprefixed
//! replies are read alike.

pub mod auth;
pub mod backend;
pub mod device;
pub mod discovery;
pub mod error;
pub mod events;
pub mod media;
pub mod observe_bridge;
pub mod ptz;
pub mod session;
pub mod soap;

use async_trait::async_trait;
use cctv_core::{CameraBackend, CameraConfig, CameraError, CapabilitySet, ProbeResult};
use cctv_transport::http::HttpConfig;

use crate::DriverAdapter;

pub use backend::OnvifCameraBackend;
pub use discovery::{Discoverable, OnvifDiscovery};
pub use error::OnvifError;
pub use events::{
    build_create_pull_point_subscription, build_pull_messages, build_renew, build_unsubscribe,
    format_iso_duration, parse_create_pull_point_subscription, parse_iso_duration,
    parse_pull_messages, parse_renew, subscribe_events, EventSubscription, OnvifEvent,
    PullPointConfig, RenewResponse, SubscriptionReference, EVENTS_NS, TOPIC_NS, WSA_NS, WSE_NS,
    WSNT_NS,
};
pub use ptz::{
    build_absolute_move, build_continuous_move, build_get_presets, build_goto_preset,
    build_remove_preset, build_set_preset, build_set_system_date_and_time_soap, build_stop,
    parse_presets, parse_set_preset, set_system_date_and_time,
};
pub use session::OnvifSession;
pub use soap::Credentials;

/// Driver adapter for ONVIF cameras.
#[derive(Debug, Default, Clone)]
pub struct OnvifDriverAdapter;

impl OnvifDriverAdapter {
    /// Create a new ONVIF driver adapter.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DriverAdapter for OnvifDriverAdapter {
    fn driver_key(&self) -> &'static str {
        "onvif"
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::from_slice(&[
            cctv_core::Capability::StreamH264,
            cctv_core::Capability::StreamH265,
            cctv_core::Capability::PtzMove,
            cctv_core::Capability::PtzPresets,
            cctv_core::Capability::CameraContinuous,
            cctv_core::Capability::AudioIn,
            cctv_core::Capability::AudioOut,
        ])
    }

    async fn probe(&self, config: &CameraConfig) -> Result<ProbeResult, CameraError> {
        let backend = self.attach(config).await?;
        backend.probe().await
    }

    async fn attach(&self, config: &CameraConfig) -> Result<Box<dyn CameraBackend>, CameraError> {
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
        )
        .map_err(crate::map::onvif::to_camera_error)?;
        Ok(Box::new(OnvifCameraBackend::new(session)))
    }
}
