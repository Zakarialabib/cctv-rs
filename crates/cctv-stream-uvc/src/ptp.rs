//! PTP as a polymorphic backend ("Capture & Fetch" surface).
//!
//! A PTP camera is treated by the domain as a "Capture & Fetch" surface rather
//! than a continuous stream. Invoking capture triggers hardware capture resulting in
//! a [`MediaFile`] entity.
//! Maps PTP `StoreNotReady` status to [`CameraError::MediaUnavailable`].

use async_trait::async_trait;
use cctv_core::{
    CameraBackend, CameraError, CameraObservation, CameraStateCurrent, MediaFile,
    ObservationStream, ProbeResult, StreamInfo, StreamQuality,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::observe_bridge::BroadcastObservationSource;

/// PTP device operational status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtpStatus {
    /// Device is ready for commands and capture.
    Ready,
    /// Storage is writing or locked (`StoreNotReady`).
    StoreNotReady,
    /// OS shell or another driver holds device handle (Device Busy).
    DeviceBusy,
}

use std::sync::RwLock;

/// PTP camera backend adapter.
pub struct PtpAdapter {
    id: String,
    serial_number: String,
    status: RwLock<PtpStatus>,
    file_counter: u64,
    tx: broadcast::Sender<CameraObservation>,
    seq: Arc<AtomicU64>,
}

impl PtpAdapter {
    /// Create a new PTP camera adapter.
    pub fn new(id: impl Into<String>, serial_number: impl Into<String>) -> Self {
        let (tx, _) = broadcast::channel(128);
        Self {
            id: id.into(),
            serial_number: serial_number.into(),
            status: RwLock::new(PtpStatus::Ready),
            file_counter: 0,
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

    /// Set the device operational status.
    pub fn set_status(&self, status: PtpStatus) {
        if let Ok(mut lock) = self.status.write() {
            *lock = status;
        }
        let (state, online, conf) = match status {
            PtpStatus::Ready => (CameraStateCurrent::Standby, true, 1.0),
            PtpStatus::StoreNotReady => (CameraStateCurrent::On, true, 0.8),
            PtpStatus::DeviceBusy => (CameraStateCurrent::Away, false, 0.0),
        };
        self.emit_observation(state, online, conf);
    }
}

#[async_trait]
impl CameraBackend for PtpAdapter {
    fn driver(&self) -> &'static str {
        "ptp"
    }

    fn capabilities(&self) -> cctv_core::CapabilitySet {
        cctv_core::CapabilitySet::from_slice(&[cctv_core::Capability::Camera4k])
    }

    async fn observe(&self) -> Result<ObservationStream, CameraError> {
        let rx = self.tx.subscribe();
        Ok(BroadcastObservationSource::new(rx).into_stream())
    }

    async fn probe(&self) -> Result<ProbeResult, CameraError> {
        let status = self.status.read().map(|s| *s).unwrap_or(PtpStatus::Ready);
        match status {
            PtpStatus::DeviceBusy => Err(CameraError::DeviceBusy),
            PtpStatus::StoreNotReady | PtpStatus::Ready => Ok(ProbeResult {
                vendor: "PTP Digital Camera".into(),
                model: format!("PTP DSLR ({})", self.serial_number),
                firmware: Some("2.0.0".into()),
                ip: None,
                mac: None,
                stream_url: None,
                ptz_supported: false,
                ai_supported: false,
            }),
        }
    }

    /// A PTP camera is not a continuous video stream; calling `stream` reports not implemented.
    async fn stream(&self, _quality: StreamQuality) -> Result<StreamInfo, CameraError> {
        Err(CameraError::NotImplemented)
    }

    /// Hardware capture trigger resulting in a [`MediaFile`] entity.
    async fn trigger_capture(&self) -> Result<MediaFile, CameraError> {
        let status = self.status.read().map(|s| *s).unwrap_or(PtpStatus::Ready);
        match status {
            PtpStatus::DeviceBusy => Err(CameraError::DeviceBusy),
            PtpStatus::StoreNotReady => Err(CameraError::MediaUnavailable),
            PtpStatus::Ready => {
                let id = format!("{}_IMG_{:04}", self.id, self.file_counter + 1);
                let filename = format!("IMG_{:04}.JPG", self.file_counter + 1);
                let bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0xFF, 0xD9];
                Ok(MediaFile {
                    id,
                    filename,
                    bytes,
                    mime_type: "image/jpeg".into(),
                    created_at: 1700000000,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ptp_adapter_returns_media_file_on_trigger_capture() {
        let adapter = PtpAdapter::new("ptp1", "SN12345");
        let media = adapter.trigger_capture().await.unwrap();

        assert_eq!(media.filename, "IMG_0001.JPG");
        assert_eq!(media.mime_type, "image/jpeg");
    }

    #[tokio::test]
    async fn ptp_adapter_maps_store_not_ready_to_media_unavailable() {
        let adapter = PtpAdapter::new("ptp1", "SN12345");
        adapter.set_status(PtpStatus::StoreNotReady);

        let err = adapter.trigger_capture().await.unwrap_err();
        assert_eq!(err, CameraError::MediaUnavailable);
    }

    #[tokio::test]
    async fn ptp_adapter_maps_device_busy_correctly() {
        let adapter = PtpAdapter::new("ptp1", "SN12345");
        adapter.set_status(PtpStatus::DeviceBusy);

        let err = adapter.trigger_capture().await.unwrap_err();
        assert_eq!(err, CameraError::DeviceBusy);
    }
}
