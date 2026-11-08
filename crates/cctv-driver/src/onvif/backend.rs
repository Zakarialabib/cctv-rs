//! One ONVIF camera, as a [`CameraBackend`].
//!
//! The session in [`super::session`] is the wire; this is the device-model
//! surface the facade and the conformance suite drive.  It picks the profile to
//! address (ONVIF's media profile token, not the camera), so the rest of the
//! workspace never sees one.

use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::broadcast;

use super::error::OnvifError;
use super::media::{pick_main_profile, pick_substream_profile};
use super::observe_bridge::BroadcastObservationSource;
use super::session::{host_of, OnvifSession};
use cctv_core::{
    CameraBackend, CameraError, CameraObservation, CameraStateCurrent, ObservationStream, Preset,
    ProbeResult, PtzCommand, Snapshot, StreamInfo, StreamQuality,
};

/// One ONVIF camera.
///
/// `profile` pins the media profile to address; `None` picks the best one per
/// call (largest resolution for the main stream, smallest for the sub).  It also
/// remembers which profile last moved, because ONVIF's `Stop` names a profile and
/// a device-model [`PtzCommand::Stop`] does not.
pub struct OnvifCameraBackend {
    session: OnvifSession,
    profile: Option<String>,
    started: Mutex<Option<String>>,
    tx: broadcast::Sender<CameraObservation>,
    seq: Arc<AtomicU64>,
}

impl OnvifCameraBackend {
    /// Bind to a device, picking the best media profile per call.
    pub fn new(session: OnvifSession) -> Self {
        let (tx, _) = broadcast::channel(128);
        Self {
            session,
            profile: None,
            started: Mutex::new(None),
            tx,
            seq: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Emit a state observation to subscribers.
    pub fn emit_observation(&self, state: CameraStateCurrent, online: bool, confidence: f32) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let cam_id = host_of(self.session.device_url()).unwrap_or_else(|| "onvif".to_string());
        let obs = CameraObservation::new(cam_id, seq, now, state, online, confidence);
        let _ = self.tx.send(obs);
    }

    /// Bind to one media profile by token.
    pub fn with_profile(mut self, profile: impl Into<String>) -> Self {
        self.profile = Some(profile.into());
        self
    }

    /// The session this backend drives.
    pub fn session(&self) -> &OnvifSession {
        &self.session
    }

    /// The profile token to address.
    async fn profile_token(&self) -> Result<String, CameraError> {
        if let Some(token) = &self.profile {
            return Ok(token.clone());
        }
        let profiles = self.session.profiles().await.map_err(camera_error)?;
        pick_main_profile(&profiles)
            .map(|profile| profile.token.clone())
            .ok_or_else(|| CameraError::from(OnvifError::NoProfile))
    }

    /// Remember the profile that started moving, so `Stop` can name it.
    fn remember_started(&self, token: &str) -> Result<(), CameraError> {
        *self.started.lock().map_err(|_| poisoned())? = Some(token.to_string());
        Ok(())
    }
}

/// Map a vendor error through the driver's own taxonomy.
fn camera_error(error: OnvifError) -> CameraError {
    crate::map::onvif::to_camera_error(&error)
}

fn poisoned() -> CameraError {
    CameraError::Other("onvif ptz state lock poisoned".into())
}

#[async_trait]
impl CameraBackend for OnvifCameraBackend {
    fn driver(&self) -> &'static str {
        "onvif"
    }

    async fn observe(&self) -> Result<ObservationStream, CameraError> {
        let rx = self.tx.subscribe();
        Ok(BroadcastObservationSource::new(rx).into_stream())
    }

    fn capabilities(&self) -> cctv_core::CapabilitySet {
        cctv_core::CapabilitySet::from_slice(&[
            cctv_core::Capability::StreamH264,
            cctv_core::Capability::StreamH265,
            cctv_core::Capability::PtzMove,
            cctv_core::Capability::PtzPresets,
            cctv_core::Capability::CameraContinuous,
            cctv_core::Capability::AudioIn,
            cctv_core::Capability::AudioOut,
        ])
    }

    async fn probe(&self) -> Result<ProbeResult, CameraError> {
        {
            let info = match self.session.device_information().await {
                Ok(info) => info,
                Err(e) => {
                    self.emit_observation(CameraStateCurrent::Away, false, 0.0);
                    return Err(camera_error(e));
                }
            };
            let services = match self.session.services().await {
                Ok(srv) => srv,
                Err(e) => {
                    self.emit_observation(CameraStateCurrent::Away, false, 0.0);
                    return Err(camera_error(e));
                }
            };
            let result = ProbeResult {
                vendor: info.manufacturer,
                model: info.model,
                firmware: info.firmware_version,
                ip: host_of(self.session.device_url()),
                mac: None,
                stream_url: None,
                // The capability document is the only reliable statement of PTZ
                // support: a fixed camera simply has no PTZ service.
                ptz_supported: services.ptz.is_some(),
                ai_supported: false,
            };
            self.emit_observation(CameraStateCurrent::On, true, 1.0);
            Ok(result)
        }
    }

    async fn stream(&self, quality: StreamQuality) -> Result<StreamInfo, CameraError> {
        {
            let profiles = self.session.profiles().await.map_err(camera_error)?;
            let chosen = match quality {
                StreamQuality::Main => pick_main_profile(&profiles),
                StreamQuality::Sub => pick_substream_profile(&profiles),
            }
            .ok_or_else(|| CameraError::from(OnvifError::NoProfile))?;
            let token = chosen.token.clone();
            let uri = self
                .session
                .stream_uri(&token)
                .await
                .map_err(camera_error)?;
            Ok(StreamInfo {
                uri,
                profile: token,
                quality,
                width: chosen.width,
                height: chosen.height,
                codec: chosen.encoding.clone(),
            })
        }
    }

    async fn snapshot(&self) -> Result<Snapshot, CameraError> {
        {
            let token = self.profile_token().await?;
            let uri = self
                .session
                .snapshot_uri(&token)
                .await
                .map_err(camera_error)?;
            let (bytes, content_type) = self
                .session
                .snapshot_bytes(&uri)
                .await
                .map_err(camera_error)?;
            Ok(Snapshot::from_bytes(bytes, content_type))
        }
    }

    async fn ptz(&self, command: PtzCommand) -> Result<(), CameraError> {
        {
            match command {
                PtzCommand::Stop => {
                    let started = self.started.lock().map_err(|_| poisoned())?.take();
                    match started {
                        Some(token) => self
                            .session
                            .stop(&token, true, true)
                            .await
                            .map_err(camera_error),
                        // Nothing moved from this driver, and ONVIF's `Stop` names
                        // a profile: there is nothing to send.
                        None => Ok(()),
                    }
                }
                PtzCommand::Continuous { pan, tilt, zoom } => {
                    let token = self.profile_token().await?;
                    self.session
                        .continuous_move(&token, pan, tilt, zoom, None)
                        .await
                        .map_err(camera_error)?;
                    self.remember_started(&token)
                }
                PtzCommand::Absolute { pan, tilt, zoom } => {
                    let token = self.profile_token().await?;
                    self.session
                        .absolute_move(&token, pan, tilt, zoom)
                        .await
                        .map_err(camera_error)?;
                    self.remember_started(&token)
                }
            }
        }
    }

    async fn presets(&self) -> Result<Vec<Preset>, CameraError> {
        {
            let token = self.profile_token().await?;
            self.session.presets(&token).await.map_err(camera_error)
        }
    }

    async fn save_preset(&self, name: &str, id: Option<&str>) -> Result<Preset, CameraError> {
        {
            let token = self.profile_token().await?;
            let assigned = self
                .session
                .set_preset(&token, name, id)
                .await
                .map_err(camera_error)?;
            Ok(Preset {
                id: assigned,
                // ONVIF presets carry no name of their own; the label lives with
                // the caller, exactly as the device model expects.
                name: name.to_string(),
                pan: 0.0,
                tilt: 0.0,
                zoom: 0.0,
            })
        }
    }

    async fn goto_preset(&self, id: &str) -> Result<(), CameraError> {
        {
            let token = self.profile_token().await?;
            self.session
                .goto_preset(&token, id)
                .await
                .map_err(camera_error)
        }
    }

    async fn delete_preset(&self, id: &str) -> Result<(), CameraError> {
        {
            let token = self.profile_token().await?;
            self.session
                .remove_preset(&token, id)
                .await
                .map_err(camera_error)
        }
    }
}
