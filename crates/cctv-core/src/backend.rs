//! The driver contract: what a connected camera can be asked to do.
//!
//! [`CameraBackend`] is the trait every driver crate implements for one
//! connected camera.  It is object safe and runtime agnostic: each method
//! returns a [`BoxFuture`] built from `std` only, so this crate still links no
//! async runtime.  The driver brings its own I/O; the caller brings the
//! executor.
//!
//! A capability a camera does not have is a typed error
//! ([`CameraError::NotImplemented`] or [`CameraError::PtzDisabled`]) — the
//! default method bodies return exactly that, so a driver only overrides what
//! its protocol actually speaks.
//!
//! [`CameraConfig`] is the vendor-neutral connection record a driver is built
//! from.  The `driver` field is a *key* (data), not a type: the facade's
//! registry maps it to a driver crate, so no vendor name is part of this API.

use crate::prelude::*;

use core::future::Future;
use core::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::commands::Preset;
use crate::error::CameraError;
use crate::state::ProbeResult;
use crate::types::CameraType;

/// A boxed, `Send` future — the return type of every [`CameraBackend`] method.
///
/// Defined here (rather than taken from `futures`) so the contract carries no
/// dependency beyond `std`.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Which of a camera's encoded streams to address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamQuality {
    /// The full-resolution main stream.
    #[default]
    Main,
    /// The low-bandwidth sub-stream, for multi-camera walls.
    Sub,
}

/// A resolved stream: where to play it and what the device advertised for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamInfo {
    /// The playable URI (normally `rtsp://`).  **May contain credentials** —
    /// log it only through [`StreamInfo::redacted_uri`].
    pub uri: String,
    /// The device-side profile / channel identifier the URI belongs to.
    pub profile: String,
    /// Which stream this is.
    pub quality: StreamQuality,
    /// Advertised width in pixels, when the device reports it.
    pub width: Option<u32>,
    /// Advertised height in pixels, when the device reports it.
    pub height: Option<u32>,
    /// Advertised codec (`H264`, `H265`, ...), when the device reports it.
    pub codec: Option<String>,
}

impl StreamInfo {
    /// The URI with any userinfo replaced by `***:***`, safe to log.
    pub fn redacted_uri(&self) -> String {
        redact_uri(&self.uri)
    }
}

/// One still image captured from a camera.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The encoded image bytes, as the camera returned them.
    pub bytes: Vec<u8>,
    /// MIME type reported by the camera (normally `image/jpeg`).
    pub content_type: String,
    /// Width in pixels, when it could be read from the image header.
    pub width: Option<u32>,
    /// Height in pixels, when it could be read from the image header.
    pub height: Option<u32>,
}

impl Snapshot {
    /// Build a snapshot from encoded bytes, reading the dimensions from the
    /// JPEG header when the payload is a JPEG.
    pub fn from_bytes(bytes: Vec<u8>, content_type: impl Into<String>) -> Self {
        let dims = jpeg_dimensions(&bytes);
        Self {
            bytes,
            content_type: content_type.into(),
            width: dims.map(|d| d.0),
            height: dims.map(|d| d.1),
        }
    }
}

/// A PTZ instruction, in the vendor-neutral vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PtzCommand {
    /// Move continuously at a normalised velocity until [`PtzCommand::Stop`].
    ///
    /// Each axis is in `-1.0..=1.0`; positive pan is right, positive tilt is up,
    /// positive zoom is in (tele).  Drivers clamp out-of-range values.
    Continuous {
        /// Pan velocity, `-1.0` (left) to `1.0` (right).
        pan: f32,
        /// Tilt velocity, `-1.0` (down) to `1.0` (up).
        tilt: f32,
        /// Zoom velocity, `-1.0` (wide) to `1.0` (tele).
        zoom: f32,
    },
    /// Move to an absolute position.
    ///
    /// Values are passed through in the **device's native absolute space**
    /// (ONVIF generic space, ISAPI tenths of a degree, ...); each driver crate
    /// documents its own.
    Absolute {
        /// Pan coordinate in the device's absolute space.
        pan: f32,
        /// Tilt coordinate in the device's absolute space.
        tilt: f32,
        /// Zoom coordinate in the device's absolute space.
        zoom: f32,
    },
    /// Stop every pan / tilt / zoom movement.
    Stop,
}

impl PtzCommand {
    /// Map the legacy direction vocabulary (`up`, `down`, `left`, `right`,
    /// `zoom_in`, `zoom_out`, `stop`) and a `0.0..=1.0` speed onto a command.
    ///
    /// Unknown directions map to [`PtzCommand::Stop`], so a typo halts the
    /// camera instead of moving it somewhere unexpected.
    pub fn from_direction(direction: &str, speed: f32) -> Self {
        let s = speed.abs().clamp(0.0, 1.0);
        let (pan, tilt, zoom) = match direction.to_ascii_lowercase().as_str() {
            "up" => (0.0, s, 0.0),
            "down" => (0.0, -s, 0.0),
            "left" => (-s, 0.0, 0.0),
            "right" => (s, 0.0, 0.0),
            "zoom_in" => (0.0, 0.0, s),
            "zoom_out" => (0.0, 0.0, -s),
            _ => return Self::Stop,
        };
        Self::Continuous { pan, tilt, zoom }
    }
}

/// Vendor-neutral connection record for one camera.
///
/// `driver` selects the driver crate through the facade's registry; every
/// other field is protocol-agnostic.  Unknown fields in `extra` are kept, so an
/// application can store its own per-camera settings next to the connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraConfig {
    /// Stable camera identity chosen by the application.
    pub id: String,
    /// Registry key of the driver that speaks this camera's protocol.
    pub driver: String,
    /// Host name or IP address of the camera.
    pub host: String,
    /// HTTP / SOAP port; the driver's default when absent.
    #[serde(default)]
    pub port: Option<u16>,
    /// RTSP port; 554 when absent.
    #[serde(default)]
    pub rtsp_port: Option<u16>,
    /// Use HTTPS for the control API.
    #[serde(default)]
    pub https: bool,
    /// Username for the control API and the stream.
    #[serde(default)]
    pub username: Option<String>,
    /// Password for the control API and the stream.
    #[serde(default)]
    pub password: Option<String>,
    /// Video channel (1-based) for multi-channel devices; 1 when absent.
    #[serde(default)]
    pub channel: Option<u32>,
    /// Full control-service URL, overriding `host` / `port` (e.g. an ONVIF
    /// device service `XAddr` from discovery).
    #[serde(default)]
    pub service_url: Option<String>,
    /// Operator-facing label.
    #[serde(default)]
    pub label: Option<String>,
    /// What the device is.
    #[serde(default = "default_camera_type")]
    pub ty: CameraType,
    /// Accept a self-signed / mismatched HTTPS certificate (common on
    /// cameras).  Off by default.
    #[serde(default)]
    pub accept_invalid_certs: bool,
    /// Request timeout in milliseconds; the driver's default when absent.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

fn default_camera_type() -> CameraType {
    CameraType::Business
}

impl CameraConfig {
    /// A minimal config: identity, driver key and host; everything else default.
    pub fn new(id: impl Into<String>, driver: impl Into<String>, host: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            driver: driver.into(),
            host: host.into(),
            port: None,
            rtsp_port: None,
            https: false,
            username: None,
            password: None,
            channel: None,
            service_url: None,
            label: None,
            accept_invalid_certs: false,
            ty: default_camera_type(),
            timeout_ms: None,
        }
    }

    /// Builder: set the credentials.
    pub fn with_credentials(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.username = Some(username.into());
        self.password = Some(password.into());
        self
    }

    /// The channel, defaulting to 1.
    pub fn channel_or_default(&self) -> u32 {
        self.channel.unwrap_or(1).max(1)
    }

    /// `http://host[:port]` or `https://...` — the control API base URL with no
    /// trailing slash.  `default_port` is used when `port` is absent and is
    /// omitted from the URL when it is the scheme default.
    pub fn base_url(&self, default_port: u16) -> String {
        let scheme = if self.https { "https" } else { "http" };
        let port = self.port.unwrap_or(default_port);
        let scheme_default = if self.https { 443 } else { 80 };
        let host = self.host.trim_end_matches('/');
        if port == scheme_default {
            format!("{scheme}://{host}")
        } else {
            format!("{scheme}://{host}:{port}")
        }
    }

    /// The RTSP authority `host[:port]` (port omitted when 554).
    pub fn rtsp_authority(&self) -> String {
        match self.rtsp_port.unwrap_or(554) {
            554 => self.host.clone(),
            p => format!("{}:{p}", self.host),
        }
    }

    /// A copy with the password replaced by `"***"`, safe to log or return to a
    /// UI.
    pub fn redacted(&self) -> Self {
        let mut c = self.clone();
        if c.password.is_some() {
            c.password = Some("***".into());
        }
        c
    }
}

/// One connected camera, as a driver exposes it.
///
/// Every method has a default that returns the typed "not supported" error,
/// so a driver implements only what its protocol speaks.  Implementations
/// must never panic.
pub trait CameraBackend: Send + Sync {
    /// Registry key of the driver serving this camera (for diagnostics only).
    fn driver(&self) -> &'static str;

    /// Read the device identity: vendor, model, firmware, MAC, PTZ support.
    fn probe(&self) -> BoxFuture<'_, Result<ProbeResult, CameraError>>;

    /// Resolve the playable URI of a stream.
    fn stream(&self, quality: StreamQuality) -> BoxFuture<'_, Result<StreamInfo, CameraError>>;

    /// Capture one still image.
    fn snapshot(&self) -> BoxFuture<'_, Result<Snapshot, CameraError>> {
        Box::pin(async { Err(CameraError::NotImplemented) })
    }

    /// Execute a PTZ command.
    fn ptz(&self, command: PtzCommand) -> BoxFuture<'_, Result<(), CameraError>> {
        let _ = command;
        Box::pin(async { Err(CameraError::PtzDisabled) })
    }

    /// List the presets stored on the device.
    fn presets(&self) -> BoxFuture<'_, Result<Vec<Preset>, CameraError>> {
        Box::pin(async { Err(CameraError::PtzDisabled) })
    }

    /// Store the current position as a preset.  `id` overwrites that preset;
    /// `None` lets the driver choose a free slot.  Returns the stored preset.
    fn save_preset<'a>(
        &'a self,
        name: &'a str,
        id: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Preset, CameraError>> {
        let _ = (name, id);
        Box::pin(async { Err(CameraError::PtzDisabled) })
    }

    /// Move to a stored preset.
    fn goto_preset<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), CameraError>> {
        let _ = id;
        Box::pin(async { Err(CameraError::PtzDisabled) })
    }

    /// Delete a stored preset.
    fn delete_preset<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), CameraError>> {
        let _ = id;
        Box::pin(async { Err(CameraError::PtzDisabled) })
    }
}

/// Replace the userinfo of a URI with `***:***` so it is safe to log.
///
/// Only the authority is inspected — an `@` in the path is not userinfo.
pub fn redact_uri(uri: &str) -> String {
    let Some((scheme, rest)) = uri.split_once("://") else {
        return uri.to_string();
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    match rest[..authority_end].rfind('@') {
        Some(at) => format!("{scheme}://***:***@{}", &rest[at + 1..]),
        None => uri.to_string(),
    }
}

/// Inject credentials into a stream URI that has none.
///
/// Devices return stream URIs without credentials even when the stream needs
/// them.  The URI is returned unchanged when there is no username, when it is
/// not a `scheme://` URI, or when it already carries userinfo (never
/// double-injected).  `@ : / ? # %` in the credentials are percent-encoded.
pub fn with_credentials(uri: &str, username: Option<&str>, password: Option<&str>) -> String {
    let Some(user) = username.filter(|u| !u.is_empty()) else {
        return uri.to_string();
    };
    let Some((scheme, rest)) = uri.split_once("://") else {
        return uri.to_string();
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    if rest[..authority_end].contains('@') {
        return uri.to_string();
    }
    let mut auth = encode_userinfo(user);
    if let Some(pass) = password.filter(|p| !p.is_empty()) {
        auth.push(':');
        auth.push_str(&encode_userinfo(pass));
    }
    format!("{scheme}://{auth}@{rest}")
}

fn encode_userinfo(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '%' => out.push_str("%25"),
            ':' => out.push_str("%3A"),
            '@' => out.push_str("%40"),
            '/' => out.push_str("%2F"),
            '?' => out.push_str("%3F"),
            '#' => out.push_str("%23"),
            c => out.push(c),
        }
    }
    out
}

/// Read `(width, height)` from a baseline or progressive JPEG header.
///
/// Returns `None` for anything that is not a well-formed JPEG; never panics.
pub fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    while i + 3 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        // Fill bytes and stand-alone markers carry no length.
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        let len = usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
        let is_sof = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            let h = bytes.get(i + 5..i + 7)?;
            let w = bytes.get(i + 7..i + 9)?;
            let height = u32::from(u16::from_be_bytes([h[0], h[1]]));
            let width = u32::from(u16::from_be_bytes([w[0], w[1]]));
            return Some((width, height));
        }
        if len < 2 {
            return None;
        }
        i += 2 + len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_direction_maps_the_legacy_vocabulary() {
        assert_eq!(
            PtzCommand::from_direction("UP", 0.8),
            PtzCommand::Continuous {
                pan: 0.0,
                tilt: 0.8,
                zoom: 0.0
            }
        );
        assert_eq!(
            PtzCommand::from_direction("left", 2.0),
            PtzCommand::Continuous {
                pan: -1.0,
                tilt: 0.0,
                zoom: 0.0
            }
        );
        assert_eq!(
            PtzCommand::from_direction("zoom_out", 0.5),
            PtzCommand::Continuous {
                pan: 0.0,
                tilt: 0.0,
                zoom: -0.5
            }
        );
        assert_eq!(
            PtzCommand::from_direction("diagonal", 1.0),
            PtzCommand::Stop
        );
    }

    #[test]
    fn credentials_are_injected_and_encoded_once() {
        assert_eq!(
            with_credentials("rtsp://h:554/s", Some("admin"), Some("p@ss:w/rd")),
            "rtsp://admin:p%40ss%3Aw%2Frd@h:554/s"
        );
        let has = "rtsp://a:b@h/s";
        assert_eq!(with_credentials(has, Some("x"), Some("y")), has);
        assert_eq!(
            with_credentials("rtsp://h/live@main", Some("u"), Some("p")),
            "rtsp://u:p@h/live@main"
        );
        assert_eq!(
            with_credentials("rtsp://h/s", None, Some("p")),
            "rtsp://h/s"
        );
        assert_eq!(
            with_credentials("rtsp://h/s", Some("u"), None),
            "rtsp://u@h/s"
        );
        assert_eq!(
            with_credentials("not a uri", Some("u"), Some("p")),
            "not a uri"
        );
    }

    #[test]
    fn redaction_hides_userinfo_but_not_paths() {
        assert_eq!(
            redact_uri("rtsp://admin:secret@h:554/s"),
            "rtsp://***:***@h:554/s"
        );
        assert_eq!(redact_uri("rtsp://h/live@main"), "rtsp://h/live@main");
        let injected = with_credentials("rtsp://h/s", Some("admin"), Some("p@ss"));
        let safe = redact_uri(&injected);
        assert!(!safe.contains("admin") && !safe.contains("p%40ss"));
    }

    #[test]
    fn jpeg_dimensions_reads_the_sof_segment() {
        // SOI, APP0 (len 16), SOF0 (len 17) with height 480, width 640.
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        jpeg.extend_from_slice(&[0u8; 14]);
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01, 0xE0, 0x02, 0x80]);
        jpeg.extend_from_slice(&[0u8; 10]);
        assert_eq!(jpeg_dimensions(&jpeg), Some((640, 480)));
        assert_eq!(jpeg_dimensions(b"not a jpeg"), None);
        assert_eq!(jpeg_dimensions(&[0xFF, 0xD8, 0xFF, 0xC0]), None);
    }

    #[test]
    fn config_builds_base_urls_without_default_ports() {
        let mut c = CameraConfig::new("c", "drv", "10.0.0.5");
        assert_eq!(c.base_url(80), "http://10.0.0.5");
        c.port = Some(8080);
        assert_eq!(c.base_url(80), "http://10.0.0.5:8080");
        c.https = true;
        c.port = None;
        assert_eq!(c.base_url(443), "https://10.0.0.5");
        assert_eq!(c.rtsp_authority(), "10.0.0.5");
        c.rtsp_port = Some(8554);
        assert_eq!(c.rtsp_authority(), "10.0.0.5:8554");
    }

    #[test]
    fn config_redaction_hides_the_password() {
        let c = CameraConfig::new("c", "drv", "h").with_credentials("admin", "pw");
        assert_eq!(c.redacted().password.as_deref(), Some("***"));
        assert_eq!(c.password.as_deref(), Some("pw"));
    }

    #[test]
    fn config_deserializes_with_defaults() {
        let c: CameraConfig =
            serde_json::from_str(r#"{"id":"a","driver":"d","host":"h"}"#).unwrap();
        assert_eq!(c.channel_or_default(), 1);
        assert_eq!(c.ty, CameraType::Business);
    }
}
