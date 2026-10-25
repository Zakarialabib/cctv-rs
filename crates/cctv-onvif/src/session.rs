//! The ONVIF session: one device, one HTTP client, one device-service URL.
//!
//! This is where the builders in [`crate::device`], [`crate::media`] and
//! [`crate::ptz`] meet the wire.  Each call wraps its body in
//! [`crate::soap::envelope`] with a WS-Security header and POSTs it through
//! `cctv-transport`'s shared client, which already negotiates HTTP Digest / Basic
//! for the devices that want that *in addition to* WS-Security.
//!
//! Service endpoints come from `GetCapabilities` and are cached after the first
//! call: a device answers `XAddr`s that differ from its device-service URL, and
//! every later call has to address the right one.

use std::sync::Mutex;

use cctv_transport::http::{HttpClient, HttpConfig, Method};
use cctv_transport::xml;

use crate::device::{
    build_get_capabilities, build_get_device_information, parse_capabilities,
    parse_device_information, DeviceInformation, Services,
};
use crate::error::OnvifError;
use crate::media::{
    build_get_profiles, build_get_snapshot_uri, build_get_stream_uri, parse_media_uri,
    parse_profiles, MediaProfile,
};
use crate::ptz::{
    build_absolute_move, build_continuous_move, build_get_presets, build_goto_preset,
    build_remove_preset, build_set_preset, build_stop, parse_presets, parse_set_preset,
};
use crate::soap::{envelope, Credentials};

/// The `Content-Type` ONVIF services expect.
const SOAP_CONTENT_TYPE: &str = "application/soap+xml";

/// One ONVIF device.
///
/// Cheap to share: every method takes `&self`, and the only mutable state is the
/// cached service table behind a `Mutex` that is never held across an `await`.
pub struct OnvifSession {
    http: HttpClient,
    credentials: Credentials,
    device_url: String,
    services: Mutex<Option<Services>>,
}

impl OnvifSession {
    /// Build a session against a device-service URL (the `XAddr` discovery
    /// reports, or `http://host/onvif/device_service`).
    ///
    /// `config` carries the timeout and whether to accept the self-signed
    /// certificates cameras ship.  Its `username` / `password` are ignored:
    /// ONVIF authenticates at the message level ([`Credentials`]), and passing
    /// them to the HTTP client too would answer a 401 challenge the device did
    /// not ask for.
    pub fn new(
        device_url: impl Into<String>,
        credentials: Credentials,
        config: HttpConfig,
    ) -> Result<Self, OnvifError> {
        let config = HttpConfig {
            username: None,
            password: None,
            ..config
        };
        Ok(Self {
            http: HttpClient::new(config)?,
            credentials,
            device_url: device_url.into(),
            services: Mutex::new(None),
        })
    }

    /// The device-service URL this session was built against.
    pub fn device_url(&self) -> &str {
        &self.device_url
    }

    /// POST one SOAP body to `url` and return the reply text.
    ///
    /// A SOAP fault becomes [`OnvifError::Fault`] with its reason, and a
    /// non-success status with no fault becomes [`OnvifError::Status`] — a caller
    /// never has to look at a status code or a fault document itself.
    async fn call(&self, url: &str, body: &str) -> Result<String, OnvifError> {
        let request = envelope(Some(&self.credentials.header()), body);
        let response = self
            .http
            .request(
                Method::Post,
                url,
                Some(SOAP_CONTENT_TYPE),
                Some(request.into_bytes()),
            )
            .await?;
        let text = response.text();
        if let Some(reason) = xml::soap_fault(&text) {
            return Err(OnvifError::Fault(reason));
        }
        if !response.is_success() {
            return Err(OnvifError::Status(response.status));
        }
        Ok(text)
    }

    /// `GetDeviceInformation` against the device service.
    pub async fn device_information(&self) -> Result<DeviceInformation, OnvifError> {
        let reply = self
            .call(&self.device_url, build_get_device_information())
            .await?;
        parse_device_information(&reply)
    }

    /// `GetCapabilities`, cached: the Media and PTZ `XAddr`s.
    pub async fn services(&self) -> Result<Services, OnvifError> {
        if let Some(cached) = self.services.lock().ok().and_then(|guard| guard.clone()) {
            return Ok(cached);
        }
        let reply = self
            .call(&self.device_url, build_get_capabilities())
            .await?;
        let services = parse_capabilities(&reply)?;
        if let Ok(mut guard) = self.services.lock() {
            *guard = Some(services.clone());
        }
        Ok(services)
    }

    /// The Media service URL, or [`OnvifError::MissingService`].
    async fn media_url(&self) -> Result<String, OnvifError> {
        self.services()
            .await?
            .media
            .ok_or(OnvifError::MissingService("Media"))
    }

    /// The PTZ service URL, or [`OnvifError::MissingService`].
    async fn ptz_url(&self) -> Result<String, OnvifError> {
        self.services()
            .await?
            .ptz
            .ok_or(OnvifError::MissingService("PTZ"))
    }

    /// `GetProfiles`.
    pub async fn profiles(&self) -> Result<Vec<MediaProfile>, OnvifError> {
        let reply = self
            .call(&self.media_url().await?, build_get_profiles())
            .await?;
        Ok(parse_profiles(&reply))
    }

    /// `GetStreamUri` for one profile.
    pub async fn stream_uri(&self, profile_token: &str) -> Result<String, OnvifError> {
        let reply = self
            .call(
                &self.media_url().await?,
                &build_get_stream_uri(profile_token),
            )
            .await?;
        parse_media_uri(&reply)
    }

    /// `GetSnapshotUri` for one profile.
    pub async fn snapshot_uri(&self, profile_token: &str) -> Result<String, OnvifError> {
        let reply = self
            .call(
                &self.media_url().await?,
                &build_get_snapshot_uri(profile_token),
            )
            .await?;
        parse_media_uri(&reply)
    }

    /// Fetch a snapshot's bytes from a URI `GetSnapshotUri` returned.
    ///
    /// The image is served by the device's own HTTP server, not by a SOAP
    /// service, so this is a plain `GET`.
    pub async fn snapshot_bytes(&self, uri: &str) -> Result<(Vec<u8>, String), OnvifError> {
        let response = self.http.request(Method::Get, uri, None, None).await?;
        if !response.is_success() {
            return Err(OnvifError::Status(response.status));
        }
        let content_type = response
            .content_type
            .unwrap_or_else(|| "image/jpeg".to_string());
        Ok((response.body, content_type))
    }

    /// `ContinuousMove`.
    pub async fn continuous_move(
        &self,
        profile_token: &str,
        pan: f32,
        tilt: f32,
        zoom: f32,
        timeout_secs: Option<u64>,
    ) -> Result<(), OnvifError> {
        self.call(
            &self.ptz_url().await?,
            &build_continuous_move(profile_token, pan, tilt, zoom, timeout_secs),
        )
        .await
        .map(|_| ())
    }

    /// `AbsoluteMove` in the generic position space.
    pub async fn absolute_move(
        &self,
        profile_token: &str,
        pan: f32,
        tilt: f32,
        zoom: f32,
    ) -> Result<(), OnvifError> {
        self.call(
            &self.ptz_url().await?,
            &build_absolute_move(profile_token, pan, tilt, zoom),
        )
        .await
        .map(|_| ())
    }

    /// `Stop`.
    pub async fn stop(
        &self,
        profile_token: &str,
        pan_tilt: bool,
        zoom: bool,
    ) -> Result<(), OnvifError> {
        self.call(
            &self.ptz_url().await?,
            &build_stop(profile_token, pan_tilt, zoom),
        )
        .await
        .map(|_| ())
    }

    /// `GetPresets`.
    pub async fn presets(&self, profile_token: &str) -> Result<Vec<cctv_core::Preset>, OnvifError> {
        let reply = self
            .call(&self.ptz_url().await?, &build_get_presets(profile_token))
            .await?;
        Ok(parse_presets(&reply))
    }

    /// `SetPreset`: stores the current position, returning the assigned token.
    pub async fn set_preset(
        &self,
        profile_token: &str,
        name: &str,
        preset_token: Option<&str>,
    ) -> Result<String, OnvifError> {
        let reply = self
            .call(
                &self.ptz_url().await?,
                &build_set_preset(profile_token, name, preset_token),
            )
            .await?;
        parse_set_preset(&reply)
            .ok_or_else(|| OnvifError::Parse("SetPresetResponse carried no PresetToken".into()))
    }

    /// `GotoPreset`.
    pub async fn goto_preset(
        &self,
        profile_token: &str,
        preset_token: &str,
    ) -> Result<(), OnvifError> {
        self.call(
            &self.ptz_url().await?,
            &build_goto_preset(profile_token, preset_token),
        )
        .await
        .map(|_| ())
    }

    /// `RemovePreset`.
    pub async fn remove_preset(
        &self,
        profile_token: &str,
        preset_token: &str,
    ) -> Result<(), OnvifError> {
        self.call(
            &self.ptz_url().await?,
            &build_remove_preset(profile_token, preset_token),
        )
        .await
        .map(|_| ())
    }
}

/// The host of a `scheme://host[:port]/...` URL.
///
/// The session already holds its URL, and a probe result wants the address; this
/// is the one place a driver needs to take a URL apart.
pub(crate) fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = authority.rsplit('@').next().unwrap_or(authority);
    if let Some(bracketed) = host.strip_prefix('[') {
        return bracketed.split_once(']').map(|(v6, _)| v6.to_string());
    }
    let host = host.split(':').next().unwrap_or(host);
    (!host.is_empty()).then(|| host.to_string())
}

#[cfg(test)]
mod tests {
    use super::host_of;

    #[test]
    fn the_host_is_read_from_the_authority_only() {
        assert_eq!(
            host_of("http://192.168.1.64/onvif/device_service").as_deref(),
            Some("192.168.1.64")
        );
        assert_eq!(
            host_of("https://admin:pw@cam.local:8443/onvif").as_deref(),
            Some("cam.local")
        );
        assert_eq!(host_of("http://[fe80::1]:80/x").as_deref(), Some("fe80::1"));
        assert_eq!(host_of("http:///nohost"), None);
    }
}
