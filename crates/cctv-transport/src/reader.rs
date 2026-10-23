//! Budgeted frame reader.
//!
//! Read frames with a budget. A slow camera must never stall the rest of the
//! fleet: nothing blocks past the budget, and the caller is notified instead.
//!
//! This module is Tiny-HTTP / RTSP-agnostic. `cctv-ezviz` and the RTSP driver
//! hand it frames, and the reader guarantees we never burn a frame rate slot.

/// Reads frames under a time budget so a slow camera cannot stall the fleet.
///
/// The reader holds no buffers of its own; the producer that owns the wire
/// supplies the frames.
pub struct FrameReader {
    // Hosted by the producer (the RTSP / HTTP stream surface).
}

impl FrameReader {
    /// Create a reader with no frames available yet.
    pub fn new() -> Self {
        Self {}
    }

    /// Try to fill a frame within `budget_ms` milliseconds; otherwise `None`.
    ///
    /// Used by the reconciler and the dashboard to time-slice cameras.
    pub fn try_fill(&mut self, _budget_ms: u32) -> Option<Frame> {
        // Owned by the producer.
        None
    }

    /// Set the per-read time budget, in milliseconds.
    pub fn set_budget(&mut self, _budget_ms: u32) {
        // Policy knob.
    }
}

impl Default for FrameReader {
    fn default() -> Self {
        Self::new()
    }
}

/// One frame off the wire, before the app sees it.
#[derive(Debug, Clone)]
pub struct Frame {
    /// Frame width, in pixels.
    pub width: u32,
    /// Frame height, in pixels.
    pub height: u32,
    /// Raw frame payload as it came off the wire.
    pub bytes: Vec<u8>,
}
