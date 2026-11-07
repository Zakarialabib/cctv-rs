//! Push-based observation stream for camera state changes.
//!
//! Provides the runtime-agnostic observation types and stream abstraction.
//! Drivers publish [`CameraObservation`] events over a push path, allowing
//! observers (like the reconciler) to react to state updates without polling.

use crate::prelude::*;

use core::task::{Context, Poll};
#[cfg(feature = "serde-sync")]
use serde::{Deserialize, Serialize};

use crate::error::CameraError;
use crate::state::{CameraStateCurrent, ProbeResult};

/// An observed camera state event.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct CameraObservation {
    /// Stable camera identifier.
    pub camera_id: String,
    /// Monotonic sequence number for strict per-camera ordering.
    pub sequence: u64,
    /// Timestamp in Unix milliseconds when the observation was recorded.
    pub timestamp_ms: u64,
    /// Observed current state of the device.
    pub state: CameraStateCurrent,
    /// Whether the device is online and responding.
    pub online: bool,
    /// Confidence level in this observation (0.0 .. 1.0).
    pub confidence: f32,
    /// Optional probe details if refreshed during this observation.
    pub probe: Option<ProbeResult>,
    /// Informational note or reason for the observation.
    pub message: Option<String>,
}

impl CameraObservation {
    /// Create a new camera observation with sequence and timestamp.
    pub fn new(
        camera_id: impl Into<String>,
        sequence: u64,
        timestamp_ms: u64,
        state: CameraStateCurrent,
        online: bool,
        confidence: f32,
    ) -> Self {
        Self {
            camera_id: camera_id.into(),
            sequence,
            timestamp_ms,
            state,
            online,
            confidence: confidence.clamp(0.0, 1.0),
            probe: None,
            message: None,
        }
    }

    /// Builder: attach probe result.
    pub fn with_probe(mut self, probe: ProbeResult) -> Self {
        self.probe = Some(probe);
        self
    }

    /// Builder: attach message detail.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

/// An abstract asynchronous stream of observations.
///
/// Encapsulates driver-specific push mechanisms (such as broadcast channels)
/// behind an alloc-only trait object without binding to any specific async runtime.
pub struct ObservationStream {
    inner: Box<dyn ObservationSource + Send + Sync>,
}

impl ObservationStream {
    /// Create an observation stream from an underlying source.
    pub fn new<S>(source: S) -> Self
    where
        S: ObservationSource + Send + Sync + 'static,
    {
        Self {
            inner: Box::new(source),
        }
    }

    /// Poll for the next observation.
    pub fn poll_next(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<CameraObservation, CameraError>>> {
        self.inner.poll_next_observation(cx)
    }

    /// Receive the next observation asynchronously.
    pub async fn next(&mut self) -> Option<Result<CameraObservation, CameraError>> {
        core::future::poll_fn(|cx| self.poll_next(cx)).await
    }
}

/// Trait implemented by driver-specific observation stream sources.
pub trait ObservationSource: Send + Sync {
    /// Poll for the next observation.
    fn poll_next_observation(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<CameraObservation, CameraError>>>;
}
