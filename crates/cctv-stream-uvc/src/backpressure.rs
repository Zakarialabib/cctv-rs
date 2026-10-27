//! Domain-driven backpressure: budgeted frame reader.
//!
//! Ensures that if the consumer application is slow, frames are dropped at the
//! infrastructure boundary to avoid unbounded memory buffering (OOM) and maintain
//! staleness invariants.

use cctv_core::{CameraError, FrameSource};
use std::collections::VecDeque;

/// A frame buffer item with arrival timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameItem {
    /// Frame raw bytes.
    pub bytes: Vec<u8>,
    /// Frame arrival sequence or timestamp.
    pub timestamp_ms: u64,
}

/// A budgeted frame reader wrapping a [`FrameSource`].
///
/// If consumer application is slow, it applies backpressure at the infrastructure
/// boundary—dropping oldest frames to maintain the domain's staleness invariants
/// and prevent out-of-memory errors.
pub struct BudgetedFrameReader<S> {
    source: S,
    capacity: usize,
    buffer: VecDeque<FrameItem>,
    dropped_frames: u64,
}

impl<S: FrameSource> BudgetedFrameReader<S> {
    /// Create a new budgeted frame reader with the given frame buffer capacity.
    pub fn new(source: S, capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            source,
            capacity,
            buffer: VecDeque::with_capacity(capacity),
            dropped_frames: 0,
        }
    }

    /// The maximum capacity of the frame buffer.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Total count of frames dropped due to backpressure.
    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames
    }

    /// Read a frame from the source and push it into the budgeted buffer.
    /// If the buffer is full, the oldest frame is dropped at the infrastructure boundary.
    pub async fn fetch_and_push(&mut self, timestamp_ms: u64) -> Result<(), CameraError> {
        let bytes = self.source.read_frame().await?;
        if self.buffer.len() >= self.capacity {
            self.buffer.pop_front();
            self.dropped_frames += 1;
        }
        self.buffer.push_back(FrameItem { bytes, timestamp_ms });
        Ok(())
    }

    /// Pop the newest frame (or oldest depending on staleness policy).
    pub fn pop_latest(&mut self) -> Option<FrameItem> {
        self.buffer.pop_back()
    }

    /// Pop the next frame in FIFO order.
    pub fn pop_next(&mut self) -> Option<FrameItem> {
        self.buffer.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct MockSource {
        frames: Vec<Vec<u8>>,
    }

    #[async_trait]
    impl FrameSource for MockSource {
        async fn read_frame(&mut self) -> Result<Vec<u8>, CameraError> {
            if self.frames.is_empty() {
                Err(CameraError::StreamFailed)
            } else {
                Ok(self.frames.remove(0))
            }
        }
    }

    #[tokio::test]
    async fn backpressure_drops_oldest_frames_when_capacity_exceeded() {
        let source = MockSource {
            frames: vec![vec![1], vec![2], vec![3], vec![4]],
        };
        let mut reader = BudgetedFrameReader::new(source, 2);

        reader.fetch_and_push(100).await.unwrap();
        reader.fetch_and_push(200).await.unwrap();
        reader.fetch_and_push(300).await.unwrap(); // Buffer capacity 2: frame 1 dropped

        assert_eq!(reader.dropped_frames(), 1);
        let item = reader.pop_latest().unwrap();
        assert_eq!(item.bytes, vec![3]);
        assert_eq!(item.timestamp_ms, 300);
    }
}
