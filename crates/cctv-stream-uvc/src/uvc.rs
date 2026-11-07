//! UVC OS Adapters (V4L2 for Linux, Media Foundation for Windows, AVFoundation for macOS).
//!
//! Provides the OS seam implementing [`FrameSource`] and [`CameraBackend`] without
//! exposing OS-specific APIs like IOKit or COM to the domain.

use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;

use cctv_core::{
    CameraBackend, CameraError, CameraObservation, CameraStateCurrent, FrameSource,
    ObservationStream, ProbeResult, Snapshot, StreamInfo, StreamQuality,
};

use crate::observe_bridge::BroadcastObservationSource;

/// Target OS platform for the UVC adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsPlatform {
    /// Linux V4L2 video device subsystem.
    LinuxV4l2,
    /// Windows Media Foundation / WIA device subsystem.
    WindowsMediaFoundation,
    /// macOS AVFoundation / IOKit device subsystem.
    MacOsAvFoundation,
}

/// OS-specific raw error code translated into domain errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OsError {
    /// Linux V4L2 `EBUSY` (device or resource busy).
    V4l2EBusy,
    /// Windows `MF_E_ALREADY_INITIALIZED` or COM busy code.
    MediaFoundationAlreadyInitialized,
    /// macOS AVFoundation / IOKit hardware busy code.
    AvFoundationDeviceBusy,
    /// Standard I/O or network failure.
    Io(String),
}

impl OsError {
    /// Translate OS-level busy/initialization errors into the domain's typed [`CameraError::DeviceBusy`].
    pub fn into_camera_error(self) -> CameraError {
        match self {
            OsError::V4l2EBusy
            | OsError::MediaFoundationAlreadyInitialized
            | OsError::AvFoundationDeviceBusy => CameraError::DeviceBusy,
            OsError::Io(msg) => CameraError::Other(msg),
        }
    }
}

use std::sync::atomic::AtomicBool;

/// UVC camera stream adapter.
pub struct UvcAdapter {
    id: String,
    device_path: String,
    platform: OsPlatform,
    busy: AtomicBool,
    frame_counter: u64,
    tx: broadcast::Sender<CameraObservation>,
    seq: Arc<AtomicU64>,
}

impl UvcAdapter {
    /// Create a new UVC adapter for a device node on the given OS platform.
    pub fn new(
        id: impl Into<String>,
        device_path: impl Into<String>,
        platform: OsPlatform,
    ) -> Self {
        let (tx, _) = broadcast::channel(128);
        Self {
            id: id.into(),
            device_path: device_path.into(),
            platform,
            busy: AtomicBool::new(false),
            frame_counter: 0,
            tx,
            seq: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Emit an observation to subscribers.
    pub fn emit_observation(&self, state: CameraStateCurrent, online: bool, confidence: f32) {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let obs = CameraObservation::new(&self.id, seq, now, state, online, confidence);
        let _ = self.tx.send(obs);
    }

    /// Mark the device state as held / busy by another OS shell process.
    pub fn set_busy(&self, busy: bool) {
        self.busy.store(busy, Ordering::SeqCst);
        let (state, online, conf) = if busy {
            (CameraStateCurrent::Away, false, 0.0)
        } else {
            (CameraStateCurrent::Standby, true, 1.0)
        };
        self.emit_observation(state, online, conf);
    }

    /// The camera identity.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The target OS platform of this adapter.
    pub fn platform(&self) -> OsPlatform {
        self.platform
    }
}

#[async_trait]
impl FrameSource for UvcAdapter {
    async fn read_frame(&mut self) -> Result<Vec<u8>, CameraError> {
        if self.busy.load(Ordering::SeqCst) {
            let os_err = match self.platform {
                OsPlatform::LinuxV4l2 => OsError::V4l2EBusy,
                OsPlatform::WindowsMediaFoundation => OsError::MediaFoundationAlreadyInitialized,
                OsPlatform::MacOsAvFoundation => OsError::AvFoundationDeviceBusy,
            };
            return Err(os_err.into_camera_error());
        }
        self.frame_counter += 1;
        // Mock UVC frame payload: synthetic JPEG header with counter
        let mut frame = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        frame.extend_from_slice(&self.frame_counter.to_be_bytes());
        Ok(frame)
    }
}

#[async_trait]
impl CameraBackend for UvcAdapter {
    fn driver(&self) -> &'static str {
        "uvc"
    }

    fn capabilities(&self) -> cctv_core::CapabilitySet {
        cctv_core::CapabilitySet::from_slice(&[
            cctv_core::Capability::StreamH264,
            cctv_core::Capability::CameraContinuous,
            cctv_core::Capability::AudioIn,
        ])
    }

    async fn observe(&self) -> Result<ObservationStream, CameraError> {
        let rx = self.tx.subscribe();
        Ok(BroadcastObservationSource::new(rx).into_stream())
    }

    async fn probe(&self) -> Result<ProbeResult, CameraError> {
        if self.busy.load(Ordering::SeqCst) {
            self.emit_observation(CameraStateCurrent::Away, false, 0.0);
            return Err(CameraError::DeviceBusy);
        }
        self.emit_observation(CameraStateCurrent::On, true, 1.0);
        Ok(ProbeResult {
            vendor: "UVC Standard Camera".into(),
            model: format!("{:?} ({})", self.platform, self.device_path),
            firmware: Some("1.0.0".into()),
            ip: None,
            mac: None,
            stream_url: Some(format!("uvc://{}", self.device_path)),
            ptz_supported: false,
            ai_supported: false,
        })
    }

    async fn stream(&self, quality: StreamQuality) -> Result<StreamInfo, CameraError> {
        if self.busy.load(Ordering::SeqCst) {
            return Err(CameraError::DeviceBusy);
        }
        Ok(StreamInfo {
            uri: format!("uvc://{}", self.device_path),
            profile: "uvc_main".into(),
            quality,
            width: Some(1920),
            height: Some(1080),
            codec: Some("MJPEG".into()),
        })
    }

    async fn snapshot(&self) -> Result<Snapshot, CameraError> {
        if self.busy.load(Ordering::SeqCst) {
            return Err(CameraError::DeviceBusy);
        }
        Ok(Snapshot::from_bytes(
            vec![0xFF, 0xD8, 0xFF, 0xD9],
            "image/jpeg",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn uvc_adapter_translates_os_busy_errors_to_domain_device_busy() {
        let mut adapter = UvcAdapter::new("cam1", "/dev/video0", OsPlatform::LinuxV4l2);
        adapter.set_busy(true);

        let res = adapter.read_frame().await;
        assert_eq!(res.unwrap_err(), CameraError::DeviceBusy);

        let probe_res = adapter.probe().await;
        assert_eq!(probe_res.unwrap_err(), CameraError::DeviceBusy);
    }

    #[test]
    fn os_errors_map_correctly() {
        assert_eq!(
            OsError::V4l2EBusy.into_camera_error(),
            CameraError::DeviceBusy
        );
        assert_eq!(
            OsError::MediaFoundationAlreadyInitialized.into_camera_error(),
            CameraError::DeviceBusy
        );
        assert_eq!(
            OsError::AvFoundationDeviceBusy.into_camera_error(),
            CameraError::DeviceBusy
        );
    }
}
