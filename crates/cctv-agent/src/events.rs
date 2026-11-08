//! Push-based motion and event streaming subscription service.
//!
//! Subscribes to camera motion events using ONVIF WS-Eventing / Pull-Point subscriptions,
//! delivering push-based notifications without polling loops.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use cctv_core::CameraId;
use cctv_driver::onvif::{subscribe_events, EventSubscription, OnvifSession, PullPointConfig};
use futures_core::Stream;
use tokio::sync::mpsc;

/// One detection event emitted by the motion subscription stream.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectionEvent {
    /// Identifier of the camera that detected motion.
    pub camera_id: CameraId,
    /// Timestamp in Unix milliseconds when the event was observed.
    pub timestamp_ms: u64,
    /// Event topic (e.g. `tns1:RuleEngine/Motion` or `tns1:RuleEngine/CellMotionDetector/Motion`).
    pub topic: String,
    /// Whether motion is active (`true`) or cleared/idle (`false`).
    pub is_motion: bool,
    /// Optional confidence level if available (0.0 .. 1.0).
    pub confidence: f32,
    /// Detailed attributes or metadata reported by the camera.
    pub metadata: Vec<(String, String)>,
}

impl DetectionEvent {
    /// Create a new motion detection event.
    pub fn new(
        camera_id: CameraId,
        timestamp_ms: u64,
        topic: impl Into<String>,
        is_motion: bool,
    ) -> Self {
        Self {
            camera_id,
            timestamp_ms,
            topic: topic.into(),
            is_motion,
            confidence: if is_motion { 1.0 } else { 0.0 },
            metadata: Vec::new(),
        }
    }

    /// Add metadata key-value item.
    pub fn with_metadata(mut self, key: impl Into<String>, val: impl Into<String>) -> Self {
        self.metadata.push((key.into(), val.into()));
        self
    }
}

/// Asynchronous stream of `DetectionEvent`s.
pub struct MotionEventStream {
    receiver: mpsc::Receiver<DetectionEvent>,
    subscription: EventSubscription,
}

impl MotionEventStream {
    /// Receive next event asynchronously.
    pub async fn next(&mut self) -> Option<DetectionEvent> {
        self.receiver.recv().await
    }

    /// Close the underlying subscription.
    pub fn close(&self) {
        self.subscription.close();
    }
}

impl Stream for MotionEventStream {
    type Item = DetectionEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

/// Action command triggered in response to real-time events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignageCommand {
    /// Emergency asset activation (e.g. flashing warning, alert screen, door lock).
    EmergencyAsset {
        /// Camera source id that triggered the emergency.
        camera_id: CameraId,
        /// Reason or tag for the emergency asset.
        tag: String,
    },
    /// Dismiss or revert emergency asset.
    DismissAsset {
        /// Camera source id.
        camera_id: CameraId,
    },
}

/// Service managing push-based camera event subscriptions.
#[derive(Debug, Clone)]
pub struct EventSubscriptionService {
    session: Arc<OnvifSession>,
    camera_id: CameraId,
}

impl EventSubscriptionService {
    /// Create a new event subscription service for a camera.
    pub fn new(session: Arc<OnvifSession>, camera_id: CameraId) -> Self {
        Self { session, camera_id }
    }

    /// Subscribe to motion detection events, returning an asynchronous stream.
    ///
    /// Replaces polling loops (`poll_motion()`) with push-based pull-point event streams.
    pub fn subscribe_motion(&self) -> MotionEventStream {
        let (tx, rx) = mpsc::channel(100);
        let config = PullPointConfig {
            topic_filter: Some("tns1:RuleEngine//.".into()),
            subscription_ttl: Duration::from_secs(60),
            pull_timeout: Duration::from_millis(500),
            message_limit: 10,
            channel_capacity: 100,
            initial_retry_delay: Duration::from_millis(50),
            max_retry_delay: Duration::from_secs(2),
        };

        let sub = subscribe_events(Arc::clone(&self.session), config);
        let sub_handle = sub.clone_handle();
        let cam_id = self.camera_id.clone();

        tokio::spawn(async move {
            let mut sub_events = sub;
            while let Some(ev) = sub_events.recv().await {
                let topic_lower = ev.topic.to_lowercase();
                if topic_lower.contains("motion") {
                    let is_active = ev.is_motion();
                    let det = DetectionEvent {
                        camera_id: cam_id.clone(),
                        timestamp_ms: ev.timestamp_ms,
                        topic: ev.topic,
                        is_motion: is_active,
                        confidence: if is_active { 1.0 } else { 0.0 },
                        metadata: ev.data,
                    };
                    if tx.send(det).await.is_err() {
                        break;
                    }
                }
            }
        });

        MotionEventStream {
            receiver: rx,
            subscription: sub_handle,
        }
    }
}
