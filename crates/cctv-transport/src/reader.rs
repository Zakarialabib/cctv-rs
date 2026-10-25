//! Budgeted frame reader.
//!
//! Read frames with a budget.  A slow camera must never stall the rest of the
//! fleet: nothing blocks past the budget, and the caller is told instead of
//! waiting.
//!
//! This module is RTSP- and HTTP-agnostic.  A driver's stream surface pushes
//! frames as they come off the wire; the application pulls the newest usable one
//! on its own tick.  Between them the reader does three things:
//!
//! - **bounds staleness** — a frame older than the budget is useless to a live
//!   tile, so it is dropped rather than delivered late;
//! - **bounds the backlog** — a full queue drops its oldest frame, because being
//!   behind by eight frames is worse than skipping one;
//! - **measures the stream** — received frames and bytes are averaged over a
//!   window, which is where `camera_get_stats` gets its numbers.
//!
//! The reader is **pure**: no I/O, no clock, no runtime.  The caller passes
//! `now_ms` on every call, so the measurements are exact in tests and need no
//! sleeps.  The clock only has to be monotonic and shared — use
//! `cctv_rs::clock_ms()` in the application.

use std::collections::VecDeque;

/// The default staleness budget: a frame older than this is not worth delivering
/// to a live tile.
pub const DEFAULT_BUDGET_MS: u32 = 200;

/// The default backlog bound, in frames.
pub const DEFAULT_CAPACITY: usize = 8;

/// The default measurement window, in milliseconds.
pub const DEFAULT_WINDOW_MS: u64 = 2_000;

/// One frame off the wire, before the app sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Frame width, in pixels.
    pub width: u32,
    /// Frame height, in pixels.
    pub height: u32,
    /// Raw frame payload as it came off the wire.
    pub bytes: Vec<u8>,
    /// When the frame arrived, on the caller's clock (milliseconds).
    pub received_ms: u64,
}

/// Reads frames under a time budget so a slow camera cannot stall the fleet.
///
/// Not `Clone`: one reader belongs to one camera, and copying it would split the
/// measurement.  Share it behind a `Mutex` instead.
#[derive(Debug)]
pub struct FrameReader {
    budget_ms: u32,
    capacity: usize,
    window_ms: u64,
    queue: VecDeque<Frame>,
    sequence: u64,
    received: u64,
    delivered: u64,
    dropped: u64,
    starved: u64,
    /// `(at_ms, bytes)` for every frame received, pruned to the window.
    samples: VecDeque<(u64, usize)>,
}

impl FrameReader {
    /// Create a reader with the default budget, capacity and window.
    pub fn new() -> Self {
        Self {
            budget_ms: DEFAULT_BUDGET_MS,
            capacity: DEFAULT_CAPACITY,
            window_ms: DEFAULT_WINDOW_MS,
            queue: VecDeque::new(),
            sequence: 0,
            received: 0,
            delivered: 0,
            dropped: 0,
            starved: 0,
            samples: VecDeque::new(),
        }
    }

    /// Set the staleness budget, in milliseconds.
    ///
    /// A frame older than the budget is dropped instead of delivered.  `0` means
    /// "only the current instant", which in practice delivers nothing.
    pub fn set_budget(&mut self, budget_ms: u32) {
        self.budget_ms = budget_ms;
    }

    /// The staleness budget, in milliseconds.
    pub fn budget(&self) -> u32 {
        self.budget_ms
    }

    /// Set the measurement window, in milliseconds.
    ///
    /// Shorter reacts faster and reads noisier; longer is steadier and lags a
    /// stream that just stopped.  Clamped to at least one second so a rate is
    /// never divided by zero.
    pub fn set_window(&mut self, window_ms: u64) {
        self.window_ms = window_ms.max(1_000);
    }

    /// Set how many undelivered frames to keep.
    ///
    /// A full queue drops its **oldest** frame: for a live tile the newest frame
    /// is the useful one, so a backlog must not grow.
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity.max(1);
        while self.queue.len() > self.capacity {
            self.queue.pop_front();
            self.dropped += 1;
        }
    }

    /// Hand the reader a frame that just came off the wire.
    ///
    /// `now_ms` is the arrival time on the caller's clock; it is stored on the
    /// frame so a later `try_fill` can decide whether the frame is still worth
    /// delivering.
    pub fn push(&mut self, width: u32, height: u32, bytes: Vec<u8>, now_ms: u64) {
        self.sequence += 1;
        self.received += 1;
        self.samples.push_back((now_ms, bytes.len()));
        self.prune(now_ms);

        if self.queue.len() >= self.capacity {
            // Latest frame wins: the backlog is bounded and the oldest goes.
            self.queue.pop_front();
            self.dropped += 1;
        }
        self.queue.push_back(Frame {
            width,
            height,
            bytes,
            received_ms: now_ms,
        });
    }

    /// Take the next frame, if one arrived within the budget.
    ///
    /// Never blocks.  Returns `None` when nothing is queued *or* when everything
    /// queued is older than the budget — the second case is the one the budget
    /// exists for, and it is counted in [`FrameReader::starved`] so a caller can
    /// tell "no stream" from "stream too slow".
    pub fn try_fill(&mut self, now_ms: u64) -> Option<Frame> {
        let budget = u64::from(self.budget_ms);
        while let Some(front) = self.queue.front() {
            if now_ms.saturating_sub(front.received_ms) > budget {
                self.queue.pop_front();
                self.dropped += 1;
            } else {
                break;
            }
        }

        match self.queue.pop_front() {
            Some(frame) => {
                self.delivered += 1;
                Some(frame)
            }
            None => {
                self.starved += 1;
                None
            }
        }
    }

    /// Frames received per second, averaged over the window.
    ///
    /// This is the *received* rate, not the delivered one: a camera producing 30
    /// fps into a reader the application polls at 10 Hz is a 30 fps camera.  A
    /// stream that just started reads proportionally low until the window fills.
    pub fn fps(&self, now_ms: u64) -> f32 {
        let (count, _) = self.window(now_ms);
        count as f32 * 1_000.0 / self.window_ms as f32
    }

    /// Bytes received per second, averaged over the window.
    pub fn bytes_per_sec(&self, now_ms: u64) -> u64 {
        let (_, bytes) = self.window(now_ms);
        bytes * 1_000 / self.window_ms
    }

    /// How many frames have been handed to the reader.
    pub fn received(&self) -> u64 {
        self.received
    }

    /// How many frames have been handed out by [`FrameReader::try_fill`].
    pub fn delivered(&self) -> u64 {
        self.delivered
    }

    /// How many frames were dropped, whether stale or evicted by the backlog.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// How many `try_fill` calls found nothing to hand out.
    pub fn starved(&self) -> u64 {
        self.starved
    }

    /// How many frames are queued right now.
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Forget the queue, the measurements and the counters.
    ///
    /// The sequence number is reset too: after a reconnect the stream really is a
    /// new one, and a consumer keyed on sequence must not see a jump.
    pub fn reset(&mut self) {
        self.queue.clear();
        self.samples.clear();
        self.sequence = 0;
        self.received = 0;
        self.delivered = 0;
        self.dropped = 0;
        self.starved = 0;
    }

    /// Drop samples that fell out of the window.
    fn prune(&mut self, now_ms: u64) {
        let cutoff = now_ms.saturating_sub(self.window_ms);
        while let Some((at, _)) = self.samples.front() {
            if *at < cutoff {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    /// `(frames, bytes)` inside the window ending at `now_ms`.
    fn window(&self, now_ms: u64) -> (u32, u64) {
        let cutoff = now_ms.saturating_sub(self.window_ms);
        let mut count = 0u32;
        let mut bytes = 0u64;
        for (at, size) in &self.samples {
            if *at < cutoff {
                continue;
            }
            count += 1;
            bytes += *size as u64;
        }
        (count, bytes)
    }
}

impl Default for FrameReader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(bytes: usize) -> Vec<u8> {
        vec![0u8; bytes]
    }

    /// 30 fps for one second, starting at `t0`.
    fn feed_one_second(reader: &mut FrameReader, t0: u64) -> u64 {
        let mut now = t0;
        for _ in 0..30 {
            reader.push(1920, 1080, frame(100), now);
            now += 33;
        }
        now
    }

    #[test]
    fn a_fresh_reader_reports_nothing() {
        let mut reader = FrameReader::new();
        assert_eq!(reader.fps(0), 0.0);
        assert_eq!(reader.bytes_per_sec(0), 0);
        assert_eq!(reader.queued(), 0);
        assert_eq!(reader.received(), 0);
        assert!(reader.try_fill(0).is_none());
    }

    #[test]
    fn frames_are_handed_out_with_their_payload_and_arrival_time() {
        let mut reader = FrameReader::new();
        reader.push(640, 360, vec![1, 2, 3], 1_000);

        let frame = reader.try_fill(1_050).expect("a frame");
        assert_eq!(frame.width, 640);
        assert_eq!(frame.height, 360);
        assert_eq!(frame.bytes, vec![1, 2, 3]);
        assert_eq!(frame.received_ms, 1_000);
        assert_eq!(reader.delivered(), 1);
        assert_eq!(reader.queued(), 0);
    }

    #[test]
    fn a_frame_older_than_the_budget_is_dropped_not_delivered_late() {
        let mut reader = FrameReader::new();
        reader.set_budget(100);
        reader.push(1920, 1080, frame(10), 1_000);

        // 101 ms later the frame is past the budget: a live tile would rather
        // show nothing than a frame that stale.
        assert!(reader.try_fill(1_101).is_none());
        assert_eq!(reader.dropped(), 1);
        assert_eq!(reader.delivered(), 0);
        assert_eq!(reader.starved(), 1);

        // Exactly on the budget is still deliverable.
        let mut reader = FrameReader::new();
        reader.set_budget(100);
        reader.push(1920, 1080, frame(10), 1_000);
        assert!(reader.try_fill(1_100).is_some());
    }

    #[test]
    fn the_backlog_is_bounded_and_the_oldest_frame_goes() {
        let mut reader = FrameReader::new();
        reader.set_capacity(3);
        for i in 0..5u8 {
            reader.push(16, 16, vec![i], 1_000 + u64::from(i));
        }

        assert_eq!(reader.queued(), 3);
        assert_eq!(reader.dropped(), 2, "the two oldest were evicted");
        // What survives is the newest three, in order.
        let first = reader.try_fill(1_004).expect("a frame");
        assert_eq!(first.bytes, vec![2]);
        assert_eq!(reader.try_fill(1_004).expect("a frame").bytes, vec![3]);
        assert_eq!(reader.try_fill(1_004).expect("a frame").bytes, vec![4]);
        assert!(reader.try_fill(1_004).is_none());
    }

    #[test]
    fn fps_is_the_received_rate_over_the_window() {
        let mut reader = FrameReader::new();
        let end = feed_one_second(&mut reader, 10_000);
        // 30 frames over a 2 s window = 15 fps.
        assert!((reader.fps(end) - 15.0).abs() < 0.01, "{}", reader.fps(end));

        // A second of 30 fps on top fills the window: 60 frames / 2 s.
        let end = feed_one_second(&mut reader, end);
        assert!((reader.fps(end) - 30.0).abs() < 0.01, "{}", reader.fps(end));
    }

    #[test]
    fn bytes_per_sec_is_the_received_rate_over_the_window() {
        let mut reader = FrameReader::new();
        let end = feed_one_second(&mut reader, 10_000);
        // 30 frames x 100 bytes over 2 s = 1500 B/s.
        assert_eq!(reader.bytes_per_sec(end), 1_500);
    }

    #[test]
    fn a_stream_that_stopped_ages_out_of_the_window() {
        let mut reader = FrameReader::new();
        let end = feed_one_second(&mut reader, 10_000);
        assert!(reader.fps(end) > 0.0);

        // Ten seconds later nothing is inside the 2 s window.
        assert_eq!(reader.fps(end + 10_000), 0.0);
        assert_eq!(reader.bytes_per_sec(end + 10_000), 0);
    }

    #[test]
    fn the_window_is_configurable_and_never_divides_by_zero() {
        let mut reader = FrameReader::new();
        reader.set_window(0);
        assert_eq!(reader.window_ms, 1_000, "clamped");
        assert_eq!(reader.fps(0), 0.0);
        assert_eq!(reader.bytes_per_sec(0), 0);

        reader.set_window(1_000);
        let end = feed_one_second(&mut reader, 0);
        assert!((reader.fps(end) - 30.0).abs() < 0.01, "{}", reader.fps(end));
    }

    #[test]
    fn dropping_the_budget_below_the_frame_interval_starves() {
        let mut reader = FrameReader::new();
        reader.set_budget(0);
        reader.push(16, 16, frame(1), 5_000);
        assert!(
            reader.try_fill(5_000).is_some(),
            "same instant is in budget"
        );
        reader.push(16, 16, frame(1), 5_000);
        assert!(
            reader.try_fill(5_001).is_none(),
            "1 ms late is out of budget"
        );
        assert_eq!(reader.starved(), 1);
    }

    #[test]
    fn starvation_is_counted_so_no_stream_differs_from_a_slow_one() {
        let mut reader = FrameReader::new();
        assert!(reader.try_fill(0).is_none());
        assert!(reader.try_fill(1).is_none());
        assert_eq!(reader.starved(), 2);
        assert_eq!(reader.dropped(), 0, "nothing was dropped; nothing arrived");
    }

    #[test]
    fn reset_clears_the_stream_and_its_measurements() {
        let mut reader = FrameReader::new();
        let end = feed_one_second(&mut reader, 0);
        reader.try_fill(end);
        assert!(reader.received() > 0);

        reader.reset();
        assert_eq!(reader.received(), 0);
        assert_eq!(reader.delivered(), 0);
        assert_eq!(reader.dropped(), 0);
        assert_eq!(reader.starved(), 0);
        assert_eq!(reader.queued(), 0);
        assert_eq!(reader.fps(end), 0.0);
        assert_eq!(reader.bytes_per_sec(end), 0);
    }

    #[test]
    fn capacity_zero_is_treated_as_one_rather_than_panicking() {
        let mut reader = FrameReader::new();
        reader.set_capacity(0);
        reader.push(16, 16, frame(1), 0);
        assert_eq!(reader.queued(), 1);
    }
}
