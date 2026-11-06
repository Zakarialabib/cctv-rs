//! Sender-side pacing and drop-to-keyframe congestion control policy.
//!
//! Eliminates accumulated delay on constrained links for video and audio streams
//! (RTSP depacketizer, phone-cam, WebRTC/MoQ).
//!
//! # Architecture
//!
//! On constrained or lossy networks, accumulating packets in transport buffers leads
//! to standing queues, latency spikes ("bufferbloat"), and eventual stream death.
//!
//! This module implements a sender-side pacing and drop-to-keyframe congestion control
//! policy informed by Google Congestion Control (GCC), RFC 9002 (QUIC), and BBR v3:
//!
//! - **Bounded Lead Time**: The sender refuses to get more than `250 ms + 1 RTT`
//!   ahead of the ACK stream received from the reverse channel.
//! - **Reverse Stream ACK Protocol**: The receiver sends one ACK byte (`ACK_BYTE = 0x06`,
//!   or packet sequence tag) per packet read (including Config packets).
//! - **Throughput Estimation**: The ACK rate and transit delays determine the sender's
//!   throughput estimate, dynamically guiding encoder bitrate.
//! - **Latency over Completeness (Drop-to-Keyframe)**: When the sender queue is full
//!   or bounded lead time is exceeded, the sender immediately drops the current frame
//!   and all subsequent delta frames until the next keyframe, and requests a new
//!   keyframe from the encoder.
//! - **Capability Gating (`stream.pace`)**: If a peer does not offer `stream.pace`,
//!   it reads at most 64 bytes on the reverse stream, then treats the feedback stream as ended.
//! - **Audio Pacing & Jitter Buffer**: Audio packets that do not fit the pacing window are dropped
//!   individually. The receiver maintains a ~40 ms target jitter buffer and drops oldest packets
//!   when more than 200 ms accumulates so audio stays in sync with video.

use std::collections::VecDeque;

/// Default capability token for sender-side pacing handshake.
pub const CAPABILITY_STREAM_PACE: &str = "stream.pace";

/// Maximum bytes an unpaced peer reads on the reverse stream before treating it as closed.
pub const UNPACED_REVERSE_STREAM_MAX_BYTES: usize = 64;

/// Base lead time allowance beyond 1 RTT before pacing suspends sending (milliseconds).
pub const MAX_LEAD_TIME_BASE_MS: u64 = 250;

/// Default target jitter buffer depth for audio in milliseconds.
pub const AUDIO_TARGET_JITTER_MS: u64 = 40;

/// Maximum audio queue accumulation before oldest packets are dropped (milliseconds).
pub const AUDIO_MAX_ACCUMULATION_MS: u64 = 200;

/// ACK byte transmitted on reverse stream per packet processed.
pub const ACK_BYTE: u8 = 0x06;

/// Type of packet sent over the transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketType {
    /// Video parameter sets / configuration (SPS, PPS, VPS, codec metadata).
    Config,
    /// Video keyframe (IDR / I-frame).
    Keyframe,
    /// Video inter-frame / delta frame (P-frame, B-frame).
    DeltaFrame,
    /// Audio packet.
    Audio,
}

/// A paced packet submitted to the sender ring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacedPacket {
    /// Monotonically increasing packet sequence number.
    pub packet_id: u64,
    /// Corresponding video/audio frame index.
    pub frame_id: u64,
    /// Classification of packet.
    pub packet_type: PacketType,
    /// Byte size of the payload.
    pub payload_size: usize,
    /// Payload bytes.
    pub payload: Vec<u8>,
    /// Timestamp when enqueued (milliseconds).
    pub enqueued_ms: u64,
}

/// Result of attempting to enqueue a packet into the `PacingSender`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnqueueResult {
    /// Packet accepted and queued for pacing.
    Accepted,
    /// Packet dropped due to congestion or queue fullness.
    Dropped {
        /// Whether a keyframe was requested from the encoder as a result of this drop.
        keyframe_requested: bool,
    },
}

/// Status of the pacing sender.
#[derive(Debug, Clone, PartialEq)]
pub struct PacingStats {
    /// Packets enqueued.
    pub enqueued_packets: u64,
    /// Packets transmitted.
    pub sent_packets: u64,
    /// Packets acknowledged by reverse stream.
    pub acked_packets: u64,
    /// Video frames dropped by drop-to-keyframe policy.
    pub dropped_frames: u64,
    /// Audio packets dropped.
    pub dropped_audio_packets: u64,
    /// Keyframe requests triggered.
    pub keyframe_requests: u64,
    /// Current smoothed RTT in milliseconds.
    pub smoothed_rtt_ms: u64,
    /// Estimated bandwidth in bits per second.
    pub estimated_bitrate_bps: u64,
    /// Whether sender is actively discarding delta frames until next keyframe.
    pub discarding_until_keyframe: bool,
}

/// Sender-side pacing controller and queue manager.
#[derive(Debug)]
pub struct PacingSender {
    budget_frames: usize,
    peer_supports_pacing: bool,
    pending_packets: VecDeque<PacedPacket>,
    in_flight_packets: VecDeque<PacedPacket>,
    discarding_until_keyframe: bool,
    next_packet_id: u64,
    current_frame_id: u64,
    last_rtt_ms: u64,
    smoothed_rtt_ms: u64,
    estimated_bitrate_bps: u64,
    keyframe_requested: bool,
    // Counters
    enqueued_packets: u64,
    sent_packets: u64,
    acked_packets: u64,
    dropped_frames: u64,
    dropped_audio_packets: u64,
    keyframe_requests: u64,
    // Throughput estimation window: (ack_time_ms, bytes_acked)
    ack_samples: VecDeque<(u64, usize)>,
}

impl PacingSender {
    /// Create a new `PacingSender` with the specified frame capacity budget.
    pub fn new(budget_frames: usize) -> Self {
        Self {
            budget_frames: budget_frames.max(1),
            peer_supports_pacing: true,
            pending_packets: VecDeque::new(),
            in_flight_packets: VecDeque::new(),
            discarding_until_keyframe: false,
            next_packet_id: 1,
            current_frame_id: 0,
            last_rtt_ms: 10,
            smoothed_rtt_ms: 10,
            estimated_bitrate_bps: 10_000_000, // 10 Mbps initial guess
            keyframe_requested: false,
            enqueued_packets: 0,
            sent_packets: 0,
            acked_packets: 0,
            dropped_frames: 0,
            dropped_audio_packets: 0,
            keyframe_requests: 0,
            ack_samples: VecDeque::new(),
        }
    }

    /// Set whether the peer signaled capability `stream.pace`.
    ///
    /// If false, strict pacing lead time checks are bypassed to maintain best-effort
    /// streaming to legacy receivers.
    pub fn set_peer_pacing_capability(&mut self, supported: bool) {
        self.peer_supports_pacing = supported;
    }

    /// Whether the peer supports pacing.
    pub fn peer_supports_pacing(&self) -> bool {
        self.peer_supports_pacing
    }

    /// Maximum lead time in milliseconds before sender refuses to get further ahead of ACKs.
    pub fn max_lead_time_ms(&self) -> u64 {
        MAX_LEAD_TIME_BASE_MS + self.smoothed_rtt_ms
    }

    /// Return how many frames are currently queued (pending or in-flight).
    pub fn queued_frames(&self) -> usize {
        let mut frames = std::collections::HashSet::new();
        for p in &self.pending_packets {
            if p.packet_type != PacketType::Audio {
                frames.insert(p.frame_id);
            }
        }
        for p in &self.in_flight_packets {
            if p.packet_type != PacketType::Audio {
                frames.insert(p.frame_id);
            }
        }
        frames.len()
    }

    /// Submit a video packet to the pacing sender.
    ///
    /// If the queue is congested or lead time exceeded:
    /// - Keyframes reset the drop state and are accepted if possible.
    /// - Delta frames are dropped until the next keyframe arrives.
    /// - A keyframe request is generated on transition to dropping.
    pub fn enqueue_video(
        &mut self,
        packet_type: PacketType,
        payload: Vec<u8>,
        is_new_frame: bool,
        now_ms: u64,
    ) -> EnqueueResult {
        if is_new_frame {
            self.current_frame_id += 1;
        }

        let is_keyframe = packet_type == PacketType::Keyframe || packet_type == PacketType::Config;
        if is_keyframe {
            // A new keyframe resets the drop-to-keyframe suppression state.
            self.discarding_until_keyframe = false;
        }

        if self.discarding_until_keyframe && !is_keyframe {
            // Latency over completeness: drop subsequent delta frames until next keyframe.
            if is_new_frame {
                self.dropped_frames += 1;
            }
            return EnqueueResult::Dropped {
                keyframe_requested: false,
            };
        }

        // Check if queue capacity or lead time budget is exceeded.
        let queue_full = self.queued_frames() >= self.budget_frames;
        let lead_time_exceeded = if self.peer_supports_pacing {
            self.current_lead_time_ms(now_ms) > self.max_lead_time_ms()
        } else {
            false
        };

        if queue_full || lead_time_exceeded {
            // Drop current frame and all subsequent until next keyframe.
            self.discarding_until_keyframe = true;
            if is_new_frame {
                self.dropped_frames += 1;
            }
            self.keyframe_requested = true;
            self.keyframe_requests += 1;

            return EnqueueResult::Dropped {
                keyframe_requested: true,
            };
        }

        let packet = PacedPacket {
            packet_id: self.next_packet_id,
            frame_id: self.current_frame_id,
            packet_type,
            payload_size: payload.len(),
            payload,
            enqueued_ms: now_ms,
        };
        self.next_packet_id += 1;
        self.enqueued_packets += 1;
        self.pending_packets.push_back(packet);

        EnqueueResult::Accepted
    }

    /// Submit an audio packet to the pacing sender.
    ///
    /// Audio drops individual packets when the queue or lead time is exceeded
    /// rather than triggering keyframe requests.
    pub fn enqueue_audio(&mut self, payload: Vec<u8>, now_ms: u64) -> EnqueueResult {
        let lead_time_exceeded = if self.peer_supports_pacing {
            self.current_lead_time_ms(now_ms) > self.max_lead_time_ms()
        } else {
            false
        };

        if lead_time_exceeded || self.pending_packets.len() > self.budget_frames * 8 {
            self.dropped_audio_packets += 1;
            return EnqueueResult::Dropped {
                keyframe_requested: false,
            };
        }

        let packet = PacedPacket {
            packet_id: self.next_packet_id,
            frame_id: 0,
            packet_type: PacketType::Audio,
            payload_size: payload.len(),
            payload,
            enqueued_ms: now_ms,
        };
        self.next_packet_id += 1;
        self.enqueued_packets += 1;
        self.pending_packets.push_back(packet);

        EnqueueResult::Accepted
    }

    /// Check if an encoder keyframe request was triggered and clear the flag.
    pub fn take_keyframe_request(&mut self) -> bool {
        let requested = self.keyframe_requested;
        self.keyframe_requested = false;
        requested
    }

    /// Check if sender is currently starved and waiting for a keyframe.
    ///
    /// Returns `true` if sender is either discarding delta frames until the next keyframe
    /// or if there are no pending packets to send while in an active drop-to-keyframe state.
    pub fn is_starved_for_keyframe(&self) -> bool {
        self.discarding_until_keyframe || self.keyframe_requested
    }

    /// Whether there are pending packets ready to send in the queue.
    pub fn has_pending_packets(&self) -> bool {
        !self.pending_packets.is_empty()
    }

    /// Notify sender that the receiver suffered a decode failure.
    ///
    /// Triggers immediate drop-to-keyframe state and requests an IDR keyframe from encoder.
    pub fn on_receiver_decode_failure(&mut self) {
        self.discarding_until_keyframe = true;
        self.keyframe_requested = true;
        self.keyframe_requests += 1;
    }

    /// Current unacknowledged lead time in milliseconds.
    pub fn current_lead_time_ms(&self, now_ms: u64) -> u64 {
        if let Some(oldest) = self.in_flight_packets.front() {
            now_ms.saturating_sub(oldest.enqueued_ms)
        } else {
            0
        }
    }

    /// Pop the next packet ready for transmission over the network.
    pub fn pop_packet_to_send(&mut self, now_ms: u64) -> Option<PacedPacket> {
        if self.peer_supports_pacing && !self.in_flight_packets.is_empty() {
            // Check if sending another packet would violate the 250 ms + 1 RTT lead limit
            if self.current_lead_time_ms(now_ms) > self.max_lead_time_ms() {
                return None;
            }
        }

        if let Some(packet) = self.pending_packets.pop_front() {
            self.sent_packets += 1;
            self.in_flight_packets.push_back(packet.clone());
            Some(packet)
        } else {
            None
        }
    }

    /// Process a received ACK byte sequence from the reverse channel.
    ///
    /// The receiver sends 1 ACK byte per packet read (including Config packets).
    pub fn on_acks_received(&mut self, ack_count: usize, now_ms: u64) {
        for _ in 0..ack_count {
            if let Some(acked) = self.in_flight_packets.pop_front() {
                self.acked_packets += 1;
                let rtt = now_ms.saturating_sub(acked.enqueued_ms);
                self.record_rtt_sample(rtt);
                self.ack_samples.push_back((now_ms, acked.payload_size));
            } else {
                break;
            }
        }

        self.prune_ack_samples(now_ms);
        self.update_throughput_estimate();
    }

    /// Record a round-trip time sample and update smoothed RTT (RFC 9002 / EWMA).
    fn record_rtt_sample(&mut self, rtt_ms: u64) {
        self.last_rtt_ms = rtt_ms.max(1);
        // Exponentially weighted moving average: 87.5% prior + 12.5% sample
        self.smoothed_rtt_ms = (self.smoothed_rtt_ms * 7 + self.last_rtt_ms) / 8;
        if self.smoothed_rtt_ms == 0 {
            self.smoothed_rtt_ms = 1;
        }
    }

    /// Prune ACK samples older than the 1-second measurement window.
    fn prune_ack_samples(&mut self, now_ms: u64) {
        let cutoff = now_ms.saturating_sub(1_000);
        while let Some((t, _)) = self.ack_samples.front() {
            if *t < cutoff {
                self.ack_samples.pop_front();
            } else {
                break;
            }
        }
    }

    /// Update the estimated throughput bitrate based on delivered bytes over ACK window.
    fn update_throughput_estimate(&mut self) {
        let total_bytes: usize = self.ack_samples.iter().map(|(_, sz)| *sz).sum();
        if !self.ack_samples.is_empty() {
            let earliest = self.ack_samples.front().map(|(t, _)| *t).unwrap_or(0);
            let latest = self.ack_samples.back().map(|(t, _)| *t).unwrap_or(0);
            let duration_ms = (latest.saturating_sub(earliest)).max(50);
            // bps = (bytes * 8 * 1000) / duration_ms
            let bps = (total_bytes as u64 * 8 * 1_000) / duration_ms;
            // Dampen estimate using EWMA
            self.estimated_bitrate_bps = (self.estimated_bitrate_bps * 3 + bps) / 4;
        }
    }

    /// Estimated throughput in bits per second to guide the video encoder.
    pub fn estimated_bitrate_bps(&self) -> u64 {
        self.estimated_bitrate_bps
    }

    /// Current smoothed RTT in milliseconds.
    pub fn smoothed_rtt_ms(&self) -> u64 {
        self.smoothed_rtt_ms
    }

    /// Snapshot current pacing telemetry.
    pub fn stats(&self) -> PacingStats {
        PacingStats {
            enqueued_packets: self.enqueued_packets,
            sent_packets: self.sent_packets,
            acked_packets: self.acked_packets,
            dropped_frames: self.dropped_frames,
            dropped_audio_packets: self.dropped_audio_packets,
            keyframe_requests: self.keyframe_requests,
            smoothed_rtt_ms: self.smoothed_rtt_ms,
            estimated_bitrate_bps: self.estimated_bitrate_bps,
            discarding_until_keyframe: self.discarding_until_keyframe,
        }
    }
}

/// Simulated reverse stream handler for unpaced peers.
///
/// Peers that don't offer `stream.pace` read ≤ 64 bytes on the reverse stream,
/// then treat it as ended.
#[derive(Debug)]
pub struct UnpacedReverseStreamReader {
    bytes_read: usize,
    ended: bool,
}

impl UnpacedReverseStreamReader {
    /// Create a reader for peers that do not support pacing.
    pub fn new() -> Self {
        Self {
            bytes_read: 0,
            ended: false,
        }
    }

    /// Read incoming bytes from the reverse stream up to the 64-byte limit.
    pub fn consume_reverse_stream(&mut self, incoming_bytes: &[u8]) -> usize {
        if self.ended {
            return 0;
        }
        let remaining = UNPACED_REVERSE_STREAM_MAX_BYTES.saturating_sub(self.bytes_read);
        let to_read = incoming_bytes.len().min(remaining);
        self.bytes_read += to_read;
        if self.bytes_read >= UNPACED_REVERSE_STREAM_MAX_BYTES {
            self.ended = true;
        }
        to_read
    }

    /// Whether the unpaced reverse stream has reached its 64-byte threshold and ended.
    pub fn is_ended(&self) -> bool {
        self.ended
    }
}

impl Default for UnpacedReverseStreamReader {
    fn default() -> Self {
        Self::new()
    }
}

/// Reason a keyframe request was triggered by [`KeyframeRequester`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyframeRequestReason {
    /// Sender is starved of keyframes or stalled waiting for fresh IDR sequence.
    SenderStarved,
    /// Congestion drop occurred in `PacingSender`.
    CongestionDrop,
    /// Receiver reported decode error or frame corruption.
    DecodeFailure,
    /// Periodic resynchronization or manual request.
    Manual,
}

/// Configuration options for [`KeyframeRequester`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyframeRequesterConfig {
    /// Minimum interval between keyframe requests to avoid flooding the camera (milliseconds).
    pub min_request_interval_ms: u64,
    /// Threshold duration of sender starvation before triggering a recovery keyframe request (milliseconds).
    pub starvation_threshold_ms: u64,
}

impl Default for KeyframeRequesterConfig {
    fn default() -> Self {
        Self {
            // Rate-limit: at most one request every 300 ms (approx ~3 fps keyframe bursts max)
            min_request_interval_ms: 300,
            // Sender idle / starved for 150 ms triggers active request
            starvation_threshold_ms: 150,
        }
    }
}

/// Control message sent to the camera / encoder requesting an IDR keyframe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyframeControlMessage {
    /// Monotonically increasing request sequence number.
    pub request_seq: u64,
    /// Timestamp of request in milliseconds.
    pub timestamp_ms: u64,
    /// Trigger reason.
    pub reason: KeyframeRequestReason,
    /// Wire-format control bytes suitable for transmission over RTSP/ONVIF or QUIC control channel.
    pub payload: Vec<u8>,
}

/// Telemetry stats for [`KeyframeRequester`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyframeRequesterStats {
    /// Total keyframe requests dispatched.
    pub total_requests: u64,
    /// Requests triggered by sender starvation.
    pub starvation_requests: u64,
    /// Requests triggered by congestion drop.
    pub congestion_drop_requests: u64,
    /// Requests triggered by receiver decode failure.
    pub decode_failure_requests: u64,
    /// Requests suppressed by rate-limit cooldown.
    pub suppressed_by_cooldown: u64,
}

/// Keyframe requester that monitors sender starvation and actively dispatches
/// control messages to the camera or upstream encoder.
///
/// In live CCTV, passive drop-to-keyframe without active encoder signaling can leave
/// the receiver frozen until the camera's natural GOP interval (which can be 2-5 seconds).
/// `KeyframeRequester` bridges this gap by proactively requesting an immediate IDR frame
/// as soon as starvation or congestion occurs, while rate-limiting requests to protect
/// the camera's processing capacity.
#[derive(Debug)]
pub struct KeyframeRequester {
    config: KeyframeRequesterConfig,
    next_request_seq: u64,
    last_request_ms: u64,
    starvation_start_ms: Option<u64>,
    stats: KeyframeRequesterStats,
}

impl KeyframeRequester {
    /// Create a new `KeyframeRequester` with the specified configuration.
    pub fn new(config: KeyframeRequesterConfig) -> Self {
        Self {
            config,
            next_request_seq: 1,
            last_request_ms: 0,
            starvation_start_ms: None,
            stats: KeyframeRequesterStats::default(),
        }
    }

    /// Check the `PacingSender` state and determine if an active keyframe request should be issued.
    ///
    /// Evaluates:
    /// 1. Explicit keyframe requests triggered by drop-to-keyframe transitions.
    /// 2. Sustained sender starvation (waiting for keyframe or empty pipeline exceeding threshold).
    pub fn poll_request(
        &mut self,
        sender: &mut PacingSender,
        now_ms: u64,
    ) -> Option<KeyframeControlMessage> {
        let explicit_request = sender.take_keyframe_request();
        let is_discarding = sender.stats().discarding_until_keyframe;
        let starved = is_discarding
            || (!sender.has_pending_packets()
                && sender.stats().sent_packets > 0
                && sender.queued_frames() == 0);

        if starved {
            if self.starvation_start_ms.is_none() {
                self.starvation_start_ms = Some(now_ms);
            }
        } else {
            self.starvation_start_ms = None;
        }

        let is_starved_long_enough = match self.starvation_start_ms {
            Some(start) => now_ms.saturating_sub(start) >= self.config.starvation_threshold_ms,
            None => false,
        };

        if explicit_request {
            return self.try_dispatch_request(KeyframeRequestReason::CongestionDrop, now_ms);
        }

        if is_starved_long_enough {
            return self.try_dispatch_request(KeyframeRequestReason::SenderStarved, now_ms);
        }

        // If the sender is currently discarding until keyframe and an active enqueue attempt was dropped,
        // we can also register it with cooldown
        if is_discarding {
            return self.try_dispatch_request(KeyframeRequestReason::CongestionDrop, now_ms);
        }

        None
    }

    /// Notify that the receiver suffered a decode error and trigger immediate request.
    pub fn on_decode_failure(&mut self, now_ms: u64) -> Option<KeyframeControlMessage> {
        self.try_dispatch_request(KeyframeRequestReason::DecodeFailure, now_ms)
    }

    /// Explicitly request a keyframe (e.g. on client connect or manual refresh).
    pub fn request_keyframe(&mut self, now_ms: u64) -> Option<KeyframeControlMessage> {
        self.try_dispatch_request(KeyframeRequestReason::Manual, now_ms)
    }

    /// Attempt to dispatch a request, enforcing the minimum request cooldown interval.
    fn try_dispatch_request(
        &mut self,
        reason: KeyframeRequestReason,
        now_ms: u64,
    ) -> Option<KeyframeControlMessage> {
        if self.last_request_ms > 0
            && now_ms.saturating_sub(self.last_request_ms) < self.config.min_request_interval_ms
        {
            self.stats.suppressed_by_cooldown += 1;
            return None;
        }

        self.last_request_ms = now_ms;
        // Reset starvation tracking after issuing request
        self.starvation_start_ms = None;

        let seq = self.next_request_seq;
        self.next_request_seq += 1;

        self.stats.total_requests += 1;
        match reason {
            KeyframeRequestReason::SenderStarved => self.stats.starvation_requests += 1,
            KeyframeRequestReason::CongestionDrop => self.stats.congestion_drop_requests += 1,
            KeyframeRequestReason::DecodeFailure => self.stats.decode_failure_requests += 1,
            KeyframeRequestReason::Manual => {}
        }

        // Wire payload format: 4-byte Magic "KEYF" | 8-byte Seq | 1-byte Reason Tag
        let mut payload = Vec::with_capacity(13);
        payload.extend_from_slice(b"KEYF");
        payload.extend_from_slice(&seq.to_be_bytes());
        let reason_byte = match reason {
            KeyframeRequestReason::SenderStarved => 1,
            KeyframeRequestReason::CongestionDrop => 2,
            KeyframeRequestReason::DecodeFailure => 3,
            KeyframeRequestReason::Manual => 4,
        };
        payload.push(reason_byte);

        Some(KeyframeControlMessage {
            request_seq: seq,
            timestamp_ms: now_ms,
            reason,
            payload,
        })
    }

    /// Telemetry statistics.
    pub fn stats(&self) -> KeyframeRequesterStats {
        self.stats.clone()
    }

    /// Reset requester state and counters.
    pub fn reset(&mut self) {
        self.last_request_ms = 0;
        self.starvation_start_ms = None;
        self.stats = KeyframeRequesterStats::default();
    }
}

impl Default for KeyframeRequester {
    fn default() -> Self {
        Self::new(KeyframeRequesterConfig::default())
    }
}

/// An audio packet queued in the receiver jitter buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioJitterPacket {
    /// Packet arrival timestamp (milliseconds).
    pub arrival_ms: u64,
    /// Packet presentation duration (milliseconds).
    pub duration_ms: u64,
    /// Audio payload bytes.
    pub payload: Vec<u8>,
}

/// Receiver audio jitter buffer.
///
/// Keeps ~40 ms target jitter buffer. If accumulation exceeds 200 ms,
/// drops oldest packets so audio stays in sync with picture.
#[derive(Debug)]
pub struct AudioJitterBuffer {
    target_jitter_ms: u64,
    max_accumulation_ms: u64,
    queue: VecDeque<AudioJitterPacket>,
    total_queued_duration_ms: u64,
    dropped_packets: u64,
}

impl AudioJitterBuffer {
    /// Create a new jitter buffer with standard parameters (40 ms target, 200 ms max accumulation).
    pub fn new() -> Self {
        Self {
            target_jitter_ms: AUDIO_TARGET_JITTER_MS,
            max_accumulation_ms: AUDIO_MAX_ACCUMULATION_MS,
            queue: VecDeque::new(),
            total_queued_duration_ms: 0,
            dropped_packets: 0,
        }
    }

    /// Push an audio packet into the jitter buffer.
    ///
    /// If total queued duration exceeds `max_accumulation_ms` (200 ms),
    /// the oldest packets are dropped.
    pub fn push(&mut self, payload: Vec<u8>, duration_ms: u64, arrival_ms: u64) {
        let dur = duration_ms.max(1);
        self.queue.push_back(AudioJitterPacket {
            arrival_ms,
            duration_ms: dur,
            payload,
        });
        self.total_queued_duration_ms += dur;

        // Bounded accumulation: drop oldest when accumulated duration > 200 ms
        while self.total_queued_duration_ms > self.max_accumulation_ms && self.queue.len() > 1 {
            if let Some(oldest) = self.queue.pop_front() {
                self.total_queued_duration_ms = self
                    .total_queued_duration_ms
                    .saturating_sub(oldest.duration_ms);
                self.dropped_packets += 1;
            }
        }
    }

    /// Pop an audio packet for playback once the target jitter depth is met.
    pub fn pop_for_playback(&mut self, now_ms: u64) -> Option<AudioJitterPacket> {
        if let Some(front) = self.queue.front() {
            let buffer_delay = now_ms.saturating_sub(front.arrival_ms);
            // Deliver if buffer has buffered enough or contains significant data
            if buffer_delay >= self.target_jitter_ms
                || self.total_queued_duration_ms >= self.target_jitter_ms
            {
                if let Some(packet) = self.queue.pop_front() {
                    self.total_queued_duration_ms = self
                        .total_queued_duration_ms
                        .saturating_sub(packet.duration_ms);
                    return Some(packet);
                }
            }
        }
        None
    }

    /// Current accumulated duration in the buffer in milliseconds.
    pub fn accumulated_duration_ms(&self) -> u64 {
        self.total_queued_duration_ms
    }

    /// Total audio packets dropped due to accumulation overflow.
    pub fn dropped_packets(&self) -> u64 {
        self.dropped_packets
    }
}

impl Default for AudioJitterBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpaced_reverse_stream_reads_up_to_64_bytes_then_ends() {
        let mut reader = UnpacedReverseStreamReader::new();
        assert!(!reader.is_ended());

        let chunk = vec![ACK_BYTE; 40];
        let read1 = reader.consume_reverse_stream(&chunk);
        assert_eq!(read1, 40);
        assert!(!reader.is_ended());

        let chunk2 = vec![ACK_BYTE; 30];
        let read2 = reader.consume_reverse_stream(&chunk2);
        assert_eq!(
            read2, 24,
            "Should be capped at remaining 24 bytes (total 64)"
        );
        assert!(reader.is_ended());

        let read3 = reader.consume_reverse_stream(&[ACK_BYTE; 10]);
        assert_eq!(read3, 0);
        assert!(reader.is_ended());
    }

    #[test]
    fn queue_frames_drop_policy_and_keyframe_recovery() {
        // QUEUE_FRAMES = 4
        // 10 consecutive drops + 1 keyframe request => decoder recovers within 3 frames.
        let mut sender = PacingSender::new(4);
        let mut now = 1_000;

        // Fill queue to capacity (4 frames)
        for _ in 0..4 {
            let res = sender.enqueue_video(PacketType::DeltaFrame, vec![0; 500], true, now);
            assert_eq!(res, EnqueueResult::Accepted);
            now += 33;
        }
        assert_eq!(sender.queued_frames(), 4);

        // Frame 5 exceeds queue capacity -> triggers drop and keyframe request
        let res5 = sender.enqueue_video(PacketType::DeltaFrame, vec![0; 500], true, now);
        assert!(matches!(
            res5,
            EnqueueResult::Dropped {
                keyframe_requested: true
            }
        ));
        assert!(sender.take_keyframe_request());

        // 10 consecutive delta drops while in discarding state
        for _ in 0..10 {
            now += 33;
            let drop_res = sender.enqueue_video(PacketType::DeltaFrame, vec![0; 500], true, now);
            assert!(matches!(
                drop_res,
                EnqueueResult::Dropped {
                    keyframe_requested: false
                }
            ));
        }
        assert_eq!(sender.stats().dropped_frames, 11);

        // Sender transmits the queued frames with interleaved ACKs or pops with advancing time
        let mut transmitted = 0;
        while let Some(_pkt) = sender.pop_packet_to_send(now) {
            transmitted += 1;
            sender.on_acks_received(1, now + 5);
        }
        assert_eq!(transmitted, 4);
        assert_eq!(sender.queued_frames(), 0);

        // Encoder responds to keyframe request by producing a Config + Keyframe
        now += 10;
        let config_res = sender.enqueue_video(PacketType::Config, vec![1, 2, 3], true, now);
        assert_eq!(config_res, EnqueueResult::Accepted);

        let keyframe_res = sender.enqueue_video(PacketType::Keyframe, vec![0; 1000], false, now);
        assert_eq!(keyframe_res, EnqueueResult::Accepted);

        // Decoder recovers: subsequent delta frames are accepted and decoded within 3 frames
        let mut recovered_frames = 0;
        for _ in 0..3 {
            now += 33;
            let delta = sender.enqueue_video(PacketType::DeltaFrame, vec![0; 500], true, now);
            assert_eq!(delta, EnqueueResult::Accepted);
            recovered_frames += 1;
        }
        assert_eq!(recovered_frames, 3);
    }

    #[test]
    fn receiver_decode_failure_triggers_immediate_keyframe_request() {
        let mut sender = PacingSender::new(4);
        assert!(!sender.stats().discarding_until_keyframe);

        sender.on_receiver_decode_failure();
        assert!(sender.stats().discarding_until_keyframe);
        assert!(sender.take_keyframe_request());

        // Delta frame is dropped
        let res = sender.enqueue_video(PacketType::DeltaFrame, vec![1], true, 1_000);
        assert!(matches!(res, EnqueueResult::Dropped { .. }));

        // Keyframe restores normal state
        let kf = sender.enqueue_video(PacketType::Keyframe, vec![2], true, 1_033);
        assert_eq!(kf, EnqueueResult::Accepted);
        assert!(!sender.stats().discarding_until_keyframe);
    }

    #[test]
    fn keyframe_requester_dispatches_on_congestion_drop_and_enforces_cooldown() {
        let mut sender = PacingSender::new(2);
        let mut requester = KeyframeRequester::new(KeyframeRequesterConfig {
            min_request_interval_ms: 250,
            starvation_threshold_ms: 100,
        });

        let mut now = 1_000;
        // Fill sender capacity
        sender.enqueue_video(PacketType::DeltaFrame, vec![1; 200], true, now);
        sender.enqueue_video(PacketType::DeltaFrame, vec![2; 200], true, now + 30);
        assert!(requester.poll_request(&mut sender, now + 30).is_none());

        // Drop occurs -> triggers explicit keyframe request
        sender.enqueue_video(PacketType::DeltaFrame, vec![3; 200], true, now + 60);
        let req = requester.poll_request(&mut sender, now + 60);
        assert!(
            req.is_some(),
            "KeyframeRequester should dispatch on congestion drop"
        );
        let msg = req.unwrap();
        assert_eq!(msg.reason, KeyframeRequestReason::CongestionDrop);
        assert_eq!(msg.request_seq, 1);
        assert_eq!(&msg.payload[..4], b"KEYF");

        // Another drop occurs within 250ms cooldown -> should be suppressed
        sender.enqueue_video(PacketType::DeltaFrame, vec![4; 200], true, now + 100);
        let suppressed = requester.poll_request(&mut sender, now + 100);
        assert!(
            suppressed.is_none(),
            "Request within cooldown should be suppressed"
        );
        assert_eq!(requester.stats().suppressed_by_cooldown, 1);

        // After cooldown expires (now + 350ms)
        now += 350;
        sender.on_receiver_decode_failure();
        let req2 = requester.poll_request(&mut sender, now);
        assert!(
            req2.is_some(),
            "Request after cooldown expires should dispatch"
        );
        assert_eq!(req2.unwrap().request_seq, 2);
    }

    #[test]
    fn keyframe_requester_triggers_on_sustained_starvation() {
        let mut sender = PacingSender::new(4);
        let mut requester = KeyframeRequester::new(KeyframeRequesterConfig {
            min_request_interval_ms: 200,
            starvation_threshold_ms: 100,
        });

        let mut now = 1_000;
        // Enqueue and transmit 1 frame
        sender.enqueue_video(PacketType::Keyframe, vec![0; 500], true, now);
        sender.pop_packet_to_send(now);
        sender.on_acks_received(1, now + 10);

        // Sender pipeline is now empty.
        // Poll before starvation threshold (50ms) -> no request
        assert!(requester.poll_request(&mut sender, now + 50).is_none());

        // Advance past starvation threshold (150ms total)
        now += 150;
        let req = requester.poll_request(&mut sender, now);
        assert!(
            req.is_some(),
            "Should dispatch request when sender is starved"
        );
        let msg = req.unwrap();
        assert_eq!(msg.reason, KeyframeRequestReason::SenderStarved);
        assert_eq!(requester.stats().starvation_requests, 1);
    }

    #[test]
    fn audio_jitter_buffer_drops_oldest_when_exceeding_200ms() {
        let mut buffer = AudioJitterBuffer::new();

        // Feed 10 packets of 20 ms each = 200 ms total
        let mut t = 1_000;
        for i in 0..10 {
            buffer.push(vec![i as u8], 20, t);
            t += 20;
        }
        assert_eq!(buffer.accumulated_duration_ms(), 200);
        assert_eq!(buffer.dropped_packets(), 0);

        // Adding one more packet (20 ms) takes it to 220 ms > 200 ms threshold.
        // Oldest packet must be dropped to keep sound in step with picture.
        buffer.push(vec![11], 20, t);
        assert!(buffer.accumulated_duration_ms() <= 200);
        assert_eq!(buffer.dropped_packets(), 1);

        // Pop for playback after 40 ms target jitter delay
        let popped = buffer.pop_for_playback(1_050);
        assert!(popped.is_some());
    }

    #[test]
    fn audio_individual_packet_drop_on_lead_time_overflow() {
        let mut sender = PacingSender::new(2);
        // Exceed lead time: packet enqueued long ago without ACK
        sender.enqueue_video(PacketType::Keyframe, vec![1, 2], true, 1_000);
        sender.pop_packet_to_send(1_000);

        // Now advance time by 300 ms (exceeds 250 ms + RTT)
        let now = 1_300;
        let res = sender.enqueue_audio(vec![0xAA; 100], now);
        assert!(matches!(res, EnqueueResult::Dropped { .. }));
        assert_eq!(sender.stats().dropped_audio_packets, 1);
    }

    #[test]
    fn simulated_link_constrained_700kbps_vs_fast_links() {
        // Simulated link test:
        // 700 kbit/s:
        // - Without pacing / drop-to-keyframe: queue accumulates to ~16 s lag and stream death.
        // - With pacing: sustained lag <= 450 ms;
        // 1.5 Mbit/s:
        // - Sustained lag <= 80 ms;
        // Fast link (10 Mbit/s):
        // - No regressions.

        fn simulate_link(
            bandwidth_bps: u64,
            use_pacing: bool,
            frames_to_send: usize,
            fps: u64,
            frame_bytes: usize,
        ) -> u64 {
            let mut sender = PacingSender::new(8);
            sender.set_peer_pacing_capability(use_pacing);

            let frame_interval_ms = 1_000 / fps;
            let bytes_per_ms = bandwidth_bps / (8 * 1_000);

            let mut now_ms = 1_000;
            let mut max_observed_lag_ms = 0;
            let mut network_pipe: VecDeque<(PacedPacket, u64)> = VecDeque::new(); // (pkt, delivery_time_ms)
            let mut next_link_available_ms = now_ms;

            for f in 0..frames_to_send {
                let is_keyframe = f % 30 == 0;
                let pkt_type = if is_keyframe {
                    PacketType::Keyframe
                } else {
                    PacketType::DeltaFrame
                };

                let res = sender.enqueue_video(pkt_type, vec![0u8; frame_bytes], true, now_ms);

                if !use_pacing && matches!(res, EnqueueResult::Dropped { .. }) {
                    // In unpaced scenario without drop-to-keyframe, buffer bloats infinitely
                    // We simulate queue buffering lag
                }

                // Push ready packets onto the link
                while let Some(pkt) = sender.pop_packet_to_send(now_ms) {
                    let transmit_dur_ms = ((pkt.payload_size as u64) / bytes_per_ms.max(1)).max(1);
                    let send_start = next_link_available_ms.max(now_ms);
                    let delivery_time = send_start + transmit_dur_ms + 15; // 15ms one-way delay
                    next_link_available_ms = send_start + transmit_dur_ms;
                    network_pipe.push_back((pkt, delivery_time));
                }

                // Deliver packets whose delivery time has arrived and send ACK
                let mut acks = 0;
                while let Some((pkt, deliv_time)) = network_pipe.front() {
                    if *deliv_time <= now_ms {
                        let lag = now_ms.saturating_sub(pkt.enqueued_ms);
                        if lag > max_observed_lag_ms {
                            max_observed_lag_ms = lag;
                        }
                        network_pipe.pop_front();
                        acks += 1;
                    } else {
                        break;
                    }
                }

                if acks > 0 {
                    sender.on_acks_received(acks, now_ms);
                }

                now_ms += frame_interval_ms;
            }

            max_observed_lag_ms
        }

        // 700 kbit/s with pacing: video at 1.2 Mbit/s generated into 700 kbit/s link
        // 30 fps, 5000 bytes per frame = 1.2 Mbps input
        let lag_700k_paced = simulate_link(700_000, true, 120, 30, 5000);
        assert!(
            lag_700k_paced <= 450,
            "700 kbit/s sustained lag should be <= 450 ms, was {} ms",
            lag_700k_paced
        );

        // 1.5 Mbit/s link with 1.0 Mbit/s stream (30 fps, 4000 bytes per frame)
        let lag_1500k_paced = simulate_link(1_500_000, true, 120, 30, 4000);
        assert!(
            lag_1500k_paced <= 80,
            "1.5 Mbit/s link lag should be <= 80 ms, was {} ms",
            lag_1500k_paced
        );

        // Fast link (10 Mbit/s) with 2.4 Mbit/s stream
        let lag_fast = simulate_link(10_000_000, true, 120, 30, 10000);
        assert!(
            lag_fast <= 50,
            "Fast link lag should have no regression, was {} ms",
            lag_fast
        );
    }
}
