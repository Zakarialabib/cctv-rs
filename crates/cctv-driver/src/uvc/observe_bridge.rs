//! Bridge between `tokio::sync::broadcast` and `cctv_core::observe::ObservationSource`.

use cctv_core::error::CameraError;
use cctv_core::observe::{CameraObservation, ObservationSource, ObservationStream};
use std::future::Future;
use std::task::{Context, Poll};
use tokio::sync::broadcast;

/// Wrapper around a tokio broadcast receiver for camera observations.
pub struct BroadcastObservationSource {
    receiver: broadcast::Receiver<CameraObservation>,
}

impl BroadcastObservationSource {
    /// Create a new broadcast observation source.
    pub fn new(receiver: broadcast::Receiver<CameraObservation>) -> Self {
        Self { receiver }
    }

    /// Wrap into a boxed `ObservationStream`.
    pub fn into_stream(self) -> ObservationStream {
        ObservationStream::new(self)
    }
}

impl ObservationSource for BroadcastObservationSource {
    fn poll_next_observation(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<CameraObservation, CameraError>>> {
        loop {
            match self.receiver.try_recv() {
                Ok(obs) => return Poll::Ready(Some(Ok(obs))),
                Err(broadcast::error::TryRecvError::Empty) => {
                    // Register waker using future poll if possible, or cooperative wait.
                    // To poll asynchronously via tokio, poll the receiver's recv() future.
                    // Pinning the recv future:
                    let mut fut = std::pin::pin!(self.receiver.recv());
                    match fut.as_mut().poll(cx) {
                        Poll::Ready(Ok(obs)) => return Poll::Ready(Some(Ok(obs))),
                        Poll::Ready(Err(broadcast::error::RecvError::Lagged(_dropped_count))) => {
                            // Lagged: older messages were dropped due to backpressure.
                            // Continue loop to receive next available message.
                            continue;
                        }
                        Poll::Ready(Err(broadcast::error::RecvError::Closed)) => {
                            return Poll::Ready(None);
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                }
                Err(broadcast::error::TryRecvError::Lagged(_dropped_count)) => {
                    // Older messages dropped; loop to get next available
                    continue;
                }
                Err(broadcast::error::TryRecvError::Closed) => return Poll::Ready(None),
            }
        }
    }
}
