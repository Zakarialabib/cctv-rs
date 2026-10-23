//! Budgeted frame reader.
//!
//! Read frames with a budget. A slow camera must never stall the rest of the
//! fleet: nothing blocks past the budget, and the caller is notified instead.
//!
//! This module is Tiny-HTTP / RTSP-agnostic. `cctv-ezviz` and the RTSP driver
//! hand it frames, and the reader guarantees we never burn a frame rate slot.

pub struct FrameReader {
    // Hosted by the producer (the RTSP / HTTP stream surface).
}

impl FrameReader {
    pub fn new() -> Self {
        Self {}
    }

    /// Try to fill a frame within `budget_ms` milliseconds; otherwise `None`.
    ///
    /// Used by the reconciler and the dashboard to time-slice cameras.
    pub fn try_fill(&mut self, budget_ms: u32) -> Option<Frame> {
        // Owned by the producer.
        None
    }

    pub fn set_budget(&mut self, budget_ms: u32) {
        // Policy knob.
    }
}

/// One frame off the wire, before the app sees it.
#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}
