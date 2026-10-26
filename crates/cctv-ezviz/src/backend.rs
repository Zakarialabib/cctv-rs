//! One Ezviz camera / channel, as a [`CameraBackend`].
//!
//! This is the per-camera half of the driver — the account-level half is
//! [`crate::Driver`] — and the subject of the conformance suite.

use std::sync::Mutex;

use cctv_core::{
    BoxFuture, CameraBackend, CameraError, Preset, ProbeResult, PtzCommand, StreamInfo,
    StreamQuality,
};
use ezviz_openapi::live::VideoQuality;
use ezviz_openapi::ptz::PtzDirection;
use ezviz_openapi::EzvizClient;

use crate::error::DriverError;
use crate::ptz;

/// One Ezviz camera / channel.
///
/// It remembers the last direction it started, because the platform's `stop`
/// must name the direction that was started and a device-model
/// [`PtzCommand::Stop`] does not.
///
/// `snapshot` is deliberately left at the trait default.  The platform's capture
/// endpoint only *requests* a picture and delivers it out of band, so the driver
/// cannot return bytes — the inherited [`CameraError::NotImplemented`] says
/// exactly that, and the conformance expectations claim `snapshot: false`.
pub struct EzvizCameraBackend {
    client: EzvizClient,
    serial: String,
    channel: i32,
    started: Mutex<Option<PtzDirection>>,
}

impl EzvizCameraBackend {
    /// Bind to one camera / channel of an account.
    pub fn new(client: EzvizClient, serial: impl Into<String>, channel: i32) -> Self {
        Self {
            client,
            serial: serial.into(),
            channel,
            started: Mutex::new(None),
        }
    }

    /// The device serial this backend addresses.
    pub fn serial(&self) -> &str {
        &self.serial
    }

    /// The channel this backend addresses.
    pub fn channel(&self) -> i32 {
        self.channel
    }
}

/// Map a vendor error through the driver's own taxonomy.
fn camera_error(error: ezviz_openapi::EzvizError) -> CameraError {
    CameraError::from(DriverError::from(error))
}

/// A capability the Ezviz protocol does not have, as a typed error.
fn unsupported(what: &'static str) -> CameraError {
    CameraError::from(DriverError::Unsupported(what))
}

fn poisoned() -> CameraError {
    CameraError::Other("ezviz ptz state lock poisoned".into())
}

/// A preset id is the platform's numeric index; anything else is a typed error.
fn preset_index(id: &str) -> Result<i32, CameraError> {
    id.parse::<i32>().map_err(|_| {
        CameraError::Other(format!("ezviz preset ids are platform indices, not {id:?}"))
    })
}

impl CameraBackend for EzvizCameraBackend {
    fn driver(&self) -> &'static str {
        "ezviz"
    }

    fn probe(&self) -> BoxFuture<'_, Result<ProbeResult, CameraError>> {
        Box::pin(async move {
            let detail = self
                .client
                .device_info(&self.serial)
                .await
                .map_err(camera_error)?;
            let model = detail
                .model
                .clone()
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| detail.device_name.clone());
            Ok(ProbeResult {
                vendor: "EZVIZ".into(),
                model,
                firmware: detail
                    .extra
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                ip: None,
                mac: None,
                stream_url: None,
                // `device/info` does not report capability; a PTZ call on a
                // fixed camera fails with the platform's own error instead.
                ptz_supported: detail
                    .extra
                    .get("ptz")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                ai_supported: false,
            })
        })
    }

    fn stream(&self, quality: StreamQuality) -> BoxFuture<'_, Result<StreamInfo, CameraError>> {
        Box::pin(async move {
            let platform = match quality {
                StreamQuality::Main => VideoQuality::Hd,
                StreamQuality::Sub => VideoQuality::Fluent,
            };
            let address = self
                .client
                .live_hls_address(&self.serial, self.channel, platform)
                .await
                .map_err(camera_error)?;
            Ok(StreamInfo {
                uri: address.url,
                profile: self.channel.to_string(),
                quality,
                width: None,
                height: None,
                codec: None,
            })
        })
    }

    fn ptz(&self, command: PtzCommand) -> BoxFuture<'_, Result<(), CameraError>> {
        Box::pin(async move {
            match command {
                PtzCommand::Stop => {
                    let started = self.started.lock().map_err(|_| poisoned())?.take();
                    match started {
                        Some(direction) => self
                            .client
                            .ptz_stop(&self.serial, self.channel, direction)
                            .await
                            .map_err(camera_error),
                        // Nothing was moving from this driver, and the platform
                        // needs a direction to stop: there is nothing to send.
                        None => Ok(()),
                    }
                }
                PtzCommand::Absolute { .. } => Err(unsupported("move to an absolute position")),
                PtzCommand::Continuous { pan, tilt, zoom } => {
                    match ptz::continuous_to_direction(pan, tilt, zoom) {
                        Some((direction, speed)) => {
                            self.client
                                .ptz_start(&self.serial, self.channel, direction, speed)
                                .await
                                .map_err(camera_error)?;
                            *self.started.lock().map_err(|_| poisoned())? = Some(direction);
                            Ok(())
                        }
                        // Every axis was inside the dead zone: no movement asked.
                        None => Ok(()),
                    }
                }
            }
        })
    }

    fn presets(&self) -> BoxFuture<'_, Result<Vec<Preset>, CameraError>> {
        // The platform can add, recall and clear a preset but cannot list them.
        Box::pin(async { Err(unsupported("list the device's presets")) })
    }

    fn save_preset<'a>(
        &'a self,
        name: &'a str,
        id: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Preset, CameraError>> {
        Box::pin(async move {
            if id.is_some() {
                // The platform assigns the preset index itself, so a
                // caller-chosen id is refused rather than silently ignored.
                return Err(unsupported("store a preset under a caller-chosen id"));
            }
            let index = self
                .client
                .preset_add(&self.serial, self.channel)
                .await
                .map_err(camera_error)?;
            Ok(Preset {
                id: index.to_string(),
                // Ezviz presets are unnamed; the label lives with the caller.
                name: name.to_string(),
                pan: 0.0,
                tilt: 0.0,
                zoom: 0.0,
            })
        })
    }

    fn goto_preset<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), CameraError>> {
        Box::pin(async move {
            let index = preset_index(id)?;
            self.client
                .preset_move(&self.serial, self.channel, index)
                .await
                .map_err(camera_error)
        })
    }

    fn delete_preset<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), CameraError>> {
        Box::pin(async move {
            let index = preset_index(id)?;
            self.client
                .preset_clear(&self.serial, self.channel, index)
                .await
                .map_err(camera_error)
        })
    }
}
