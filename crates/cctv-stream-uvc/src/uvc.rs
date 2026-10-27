//! UVC OS Adapters (V4L2 for Linux, Media Foundation for Windows, AVFoundation for macOS).
//!
//! Provides the OS seam implementing [`FrameSource`] and [`CameraBackend`] without
//! exposing OS-specific APIs like IOKit or COM to the domain.

use async_trait::async_trait;
use cctv_core::{
    CameraBackend, CameraError, FrameSource, ProbeResult, Snapshot, StreamInfo, StreamQuality,
};

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

/// UVC camera stream adapter.
pub struct UvcAdapter {
    id: String,
    device_path: String,
    platform: OsPlatform,
    busy: bool,
    frame_counter: u64,
}

impl UvcAdapter {
    /// Create a new UVC adapter for a device node on the given OS platform.
    pub fn new(
        id: impl Into<String>,
        device_path: impl Into<String>,
        platform: OsPlatform,
    ) -> Self {
        Self {
            id: id.into(),
            device_path: device_path.into(),
            platform,
            busy: false,
            frame_counter: 0,
        }
    }

    /// Mark the device state as held / busy by another OS shell process.
    pub fn set_busy(&mut self, busy: bool) {
        self.busy = busy;
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
        if self.busy {
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

    async fn probe(&self) -> Result<ProbeResult, CameraError> {
        if self.busy {
            return Err(CameraError::DeviceBusy);
        }
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
        if self.busy {
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
        if self.busy {
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
