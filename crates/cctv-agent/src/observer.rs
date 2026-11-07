//! Asynchronous background subscriber for camera observations.
//!
//! Subscribes to an [`ObservationStream`] per camera and drives the reconciler
//! on every push event instead of waiting for a manual poll or periodic tick.
//! Strictly enforces monotonic sequence ordering per camera so that out-of-order
//! arrivals are rejected.

use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::reconcile::{Reconciler, Remediation};
use cctv_core::{CameraObservation, CameraState, ObservationStream};

/// A background observation subscriber driving the reconciler on push events.
pub struct ObservationSubscriber {
    reconciler: Arc<Mutex<Reconciler>>,
    last_seq: Mutex<BTreeMap<String, u64>>,
}

impl ObservationSubscriber {
    /// Create a new subscriber wrapping a shared reconciler.
    pub fn new(reconciler: Arc<Mutex<Reconciler>>) -> Self {
        Self {
            reconciler,
            last_seq: Mutex::new(BTreeMap::new()),
        }
    }

    /// Process a single incoming observation, enforcing per-camera sequence ordering.
    pub async fn process_observation(
        &self,
        obs: &CameraObservation,
        camera: &mut CameraState,
    ) -> Option<Remediation> {
        let mut seqs = self.last_seq.lock().await;
        let last = seqs.get(&obs.camera_id).copied().unwrap_or(0);
        if obs.sequence <= last {
            // Out of order observation; ignore to guarantee per-camera monotonic ordering
            return None;
        }
        seqs.insert(obs.camera_id.clone(), obs.sequence);

        let mut r = self.reconciler.lock().await;
        r.handle_observation(obs, camera)
    }

    /// Spawn a background task consuming the given [`ObservationStream`] and reconciling `camera`.
    pub fn spawn_camera_observer(
        self: Arc<Self>,
        mut stream: ObservationStream,
        camera_state: Arc<Mutex<CameraState>>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(res) = stream.next().await {
                if let Ok(obs) = res {
                    let mut cam = camera_state.lock().await;
                    self.process_observation(&obs, &mut cam).await;
                }
            }
        })
    }
}
