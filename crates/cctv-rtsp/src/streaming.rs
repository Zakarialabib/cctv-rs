//! The interleaved media loop: RTP in, access units out, frames measured.
//!
//! **Why this is not `net::feed_chunks`.** `feed_chunks` counts *bytes*, which is
//! right for a stream whose payload is the measurement.  An RTSP interleaved
//! stream is not that: every media frame carries a 4-byte `$ <channel> <len>`
//! prefix, and the media channel shares the connection with the camera's own
//! RTSP control messages.  Feeding the raw chunks to a byte counter would bill
//! the framing and the control traffic to the stream — a number that is wrong in
//! the direction that matters, because it makes a dead camera look like a
//! working one.  So the demux lives here and only the **depacketised access
//! units** reach the reader.
//!
//! **Why the reader is [`FrameReader::push`] and not `feed_chunks`.** The reader
//! measures *frames*; an RTP packet is not a frame, but an access unit is — one
//! timestamp, one marker, one decodable image.  Pushing a unit makes `fps` mean
//! frames per second rather than packets per second, which is what
//! `camera_get_stats` reports.
//!
//! **Why a dropped unit is not an error.** A lost packet makes an access unit
//! undecodable, and the depacketiser already drops it.  A live tile is better
//! off skipping a frame than showing half of one, so the loop keeps going; only
//! the *connection* failing ends the loop.

use std::sync::Arc;
use std::time::Duration;

use cctv_core::{CameraError, StreamQuality};
use cctv_rtp::h264::H264Depacketizer;
use cctv_rtp::h265::H265Depacketizer;
use cctv_rtp::parse_rtp;
use cctv_transport::net::{self, Stream};
use cctv_transport::reader::FrameReader;

use crate::backend::{
    map_net_error, map_status, read_response, RtspSession, SessionOffer, DEFAULT_RECONNECT_DELAY,
    DEFAULT_RETRIES,
};
use crate::message::Method;
use crate::sdp;

/// The chunk size the read loop drains with.
///
/// Large enough that a keyframe spread over several RTP packets is usually one
/// read, small enough that a camera sending multi-megabyte frames cannot make
/// the loop buffer without bound.  The buffer is drained per iteration, so this
/// is a throughput knob, not a memory bound.
pub const READ_CHUNK: usize = 1 << 16;

/// How long the demux loop may go without a byte before it calls the session
/// dead.
///
/// A camera that stops sending media but keeps the TCP connection open is the
/// failure mode a keepalive cannot see: `OPTIONS` is answered by the server, not
/// by the encoder.  This is the read deadline that catches it.
pub const READ_STALL: Duration = Duration::from_secs(15);

/// One demuxed access unit, ready for the reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The RTP timestamp every packet of this unit shared.
    pub timestamp: u32,
    /// The NAL units, in wire order, headers intact.
    pub nal_units: Vec<Vec<u8>>,
    /// Total bytes of NAL data — what the reader is billed for.
    pub bytes: usize,
    /// The access unit's codec (`H264` / `H265`), from the SDP.
    pub codec: String,
}

/// Why the demux loop stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamEnd {
    /// The camera closed the connection.
    Closed,
    /// Nothing arrived within [`READ_STALL`].
    Stalled,
    /// A read failed.
    Failed,
    /// The caller stopped it.
    Cancelled,
    /// The reconnect attempts ran out.
    RetriesExhausted,
}

/// The result of one streaming session, after it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamOutcome {
    /// How many access units reached the reader.
    pub frames: u64,
    /// How many units the depacketiser dropped for want of a packet.
    pub dropped: u64,
    /// Why the loop stopped.
    pub reason: StreamEnd,
}

/// What one chunk held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece<'a> {
    /// An interleaved media frame: the channel and the RTP packet.
    Media {
        /// The channel the `SETUP` assigned (0 for media).
        channel: u8,
        /// The RTP packet, without the `$` framing.
        packet: &'a [u8],
    },
    /// An RTSP control message, already parsed.
    Message(crate::message::Message),
}

/// Split a byte stream into interleaved media frames and RTSP control messages.
///
/// This is the demux the whole module is about, and it is pure so it can be
/// tested without a socket: [`split`](demux::split) returns how many bytes it
/// consumed and what it found.  `$`-framed media comes back as an RTP packet;
/// anything else is an RTSP message, which the loop handles or discards.
///
/// A partial frame at the end of the buffer is *not* an error — the caller
/// prepends the remainder to the next chunk.  Returns `None` then, meaning
/// "read more".
pub mod demux {
    use super::*;

    /// Decode one piece from the front of `buf`, returning it and its length.
    ///
    /// `Ok(None)` means the buffer does not hold a whole piece yet.
    pub fn split(buf: &[u8]) -> Result<Option<(Piece<'_>, usize)>, CameraError> {
        if buf.is_empty() {
            return Ok(None);
        }
        if buf[0] == b'$' {
            let Some(Ok((frame, used))) = crate::message::parse_interleaved(buf) else {
                return Ok(None);
            };
            return Ok(Some((
                Piece::Media {
                    channel: frame.channel,
                    packet: frame.payload,
                },
                used,
            )));
        }
        // A control message: the parser knows its own length.
        match crate::message::Message::parse(buf) {
            Ok((message, used)) => Ok(Some((Piece::Message(message), used))),
            Err(crate::error::RtspError::Incomplete) => Ok(None),
            // A malformed control header is not a media frame either; the loop
            // must not wedge on one bad line, so the caller skips it.
            Err(crate::error::RtspError::Malformed(detail)) => Err(CameraError::Other(format!(
                "malformed RTSP control message: {detail}"
            ))),
        }
    }
}

/// The depacketiser state for one stream.
///
/// Two of them, because the codec is only known after `DESCRIBE`.  Everything
/// else (MJPEG, audio) is counted as a byte stream with no frame structure,
/// which is still a measurement and is better than dropping it.
#[derive(Debug, Default)]
pub enum Depacketizer {
    /// No codec was advertised, or one this crate does not reassemble.
    #[default]
    Bytes,
    /// H.264 (RFC 6184).
    H264(H264Depacketizer),
    /// H.265 (RFC 7798).
    H265(H265Depacketizer),
}

impl Depacketizer {
    /// Build the depacketiser an encoding name implies.
    pub fn for_encoding(encoding: &str) -> Self {
        match encoding.to_ascii_uppercase().as_str() {
            "H264" | "AVC" => Self::H264(H264Depacketizer::new()),
            "H265" | "HEVC" => Self::H265(H265Depacketizer::new()),
            _ => Self::Bytes,
        }
    }

    /// Feed one RTP packet's payload; returns a completed access unit.
    pub fn push(&mut self, payload: &[u8], marker: bool, timestamp: u32) -> Option<Frame> {
        match self {
            Self::H264(inner) => inner.push(payload, marker, timestamp).map(|unit| Frame {
                timestamp: unit.timestamp,
                nal_units: unit.nal_units,
                bytes: unit.bytes,
                codec: "H264".to_string(),
            }),
            Self::H265(inner) => inner.push(payload, marker, timestamp).map(|unit| Frame {
                timestamp: unit.timestamp,
                nal_units: unit.nal_units,
                bytes: unit.bytes,
                codec: "H265".to_string(),
            }),
            // No NAL structure to recover: the payload is the frame, which is
            // true of MJPEG and of every codec this crate does not depacketise.
            Self::Bytes => {
                if payload.is_empty() {
                    None
                } else {
                    Some(Frame {
                        timestamp,
                        nal_units: vec![payload.to_vec()],
                        bytes: payload.len(),
                        codec: "bytes".to_string(),
                    })
                }
            }
        }
    }

    /// How many units were dropped for want of a packet.
    pub fn dropped(&self) -> u64 {
        match self {
            Self::H264(inner) => inner.dropped(),
            Self::H265(inner) => inner.dropped(),
            Self::Bytes => 0,
        }
    }
}

/// Push one access unit into the reader, at the caller's clock.
///
/// The width and height are the ones the codec actually declares, read from the
/// SPS when one is in band or in the out-of-band parameter sets, else `0` —
/// which the reader treats as "size only" and which is honest: a session
/// description does not carry resolution, and guessing 1920x1080 is how a 640x360
/// sub-stream gets reported as a 1080p main.
pub fn push_frame(
    reader: &mut FrameReader,
    frame: &Frame,
    parameter_sets: &[Vec<u8>],
    now_ms: u64,
) {
    let (width, height) = dimensions(frame, parameter_sets);
    let mut bytes = Vec::with_capacity(frame.bytes);
    for nal in &frame.nal_units {
        bytes.extend_from_slice(nal);
    }
    reader.push(width, height, bytes, now_ms);
}

/// Read `(width, height)` from the SPS of a completed unit.
///
/// Returns `(0, 0)` when it cannot be read — which is the truth for a stream
/// whose SPS is neither in band nor in the parameter sets.
pub fn dimensions(frame: &Frame, parameter_sets: &[Vec<u8>]) -> (u32, u32) {
    for nal in frame.nal_units.iter().chain(parameter_sets.iter()) {
        if let Some(size) = sps_dimensions(nal) {
            return size;
        }
    }
    (0, 0)
}

/// The resolution of one H.264 SPS NAL unit, parsed without a full decoder.
///
/// A minimal exp-Golomb read of the profile, the scaling lists and the cropping
/// flags.  It is deliberately *not* a decoder: it returns `None` on anything it
/// does not recognise rather than a resolution that is wrong.
fn sps_dimensions(sps: &[u8]) -> Option<(u32, u32)> {
    let header = *sps.first()?;
    if header & 0x1F != 7 {
        return None; // not an SPS
    }
    let profile_idc = *sps.get(1)?;
    let mut bits = BitReader::new(sps.get(2..)?);
    bits.ue()?; // seq_parameter_set_id
    let chroma_format_idc = if matches!(
        profile_idc,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        let idc = bits.ue().unwrap_or(1);
        if idc == 3 {
            bits.bit();
        }
        idc
    } else {
        1
    };
    bits.ue()?; // bit_depth_luma_minus8
    bits.ue()?; // bit_depth_chroma_minus8
    bits.bit(); // qpprime_y_zero_transform_bypass_flag
    if bits.bit() == 1 {
        let count = if chroma_format_idc == 3 { 12 } else { 8 };
        for index in 0..count {
            if bits.bit() == 1 {
                let size = if index < 6 { 16 } else { 64 };
                let mut last = 8u32;
                let mut next = 8u32;
                for _ in 0..size {
                    if next != 0 {
                        let delta = bits.se()?;
                        next = ((i64::from(last) + i64::from(delta) + 256) % 256) as u32;
                    }
                    last = if next != 0 { next } else { last };
                }
            }
        }
    }
    let pic_order_cnt_type = bits.ue()?;
    if pic_order_cnt_type == 0 {
        bits.ue()?;
    } else if pic_order_cnt_type == 1 {
        bits.bit();
        bits.se()?;
        bits.se()?;
        let count = bits.ue()?;
        for _ in 0..count {
            bits.se()?;
        }
    }
    bits.ue()?; // max_num_ref_frames
    bits.bit(); // gaps_in_frame_num_value_allowed_flag
    let pic_width_in_mbs_minus1 = bits.ue()?;
    let pic_height_in_map_units_minus1 = bits.ue()?;
    let frame_mbs_only_flag = bits.bit();
    if frame_mbs_only_flag == 0 {
        bits.bit(); // mb_adaptive_frame_field_flag
    }
    bits.bit(); // direct_8x8_inference_flag
    let mut crop = (0u32, 0u32, 0u32, 0u32);
    if bits.bit() == 1 {
        crop = (bits.ue()?, bits.ue()?, bits.ue()?, bits.ue()?);
    }
    let width = (pic_width_in_mbs_minus1 + 1) * 16;
    let height = (pic_height_in_map_units_minus1 + 1) * 16 * (2 - frame_mbs_only_flag);
    let crop_unit_x = if chroma_format_idc == 0 { 1 } else { 2 };
    let crop_unit_y = if chroma_format_idc == 0 {
        2 - frame_mbs_only_flag
    } else {
        2 * (2 - frame_mbs_only_flag)
    };
    let width = width.saturating_sub(crop_unit_x * (crop.0 + crop.1));
    let height = height.saturating_sub(crop_unit_y * (crop.2 + crop.3));
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

/// A minimal MSB-first bit reader over an exp-Golomb stream.
struct BitReader<'a> {
    /// The bytes after the NAL header.
    bytes: &'a [u8],
    /// The next bit index within `bytes`.
    bit: usize,
}

impl<'a> BitReader<'a> {
    /// A reader over `bytes`.
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit: 0 }
    }

    /// The next bit, `0` past the end — which is what a truncated stream reads
    /// as, and what makes a bad SPS `(0, 0)` rather than a panic.
    fn bit(&mut self) -> u32 {
        let byte = self.bytes.get(self.bit / 8).copied().unwrap_or(0);
        let value = (byte >> (7 - (self.bit % 8))) & 1;
        self.bit += 1;
        u32::from(value)
    }

    /// An unsigned exp-Golomb code (`ue`).
    fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0u32;
        while self.bit() == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        let mut value = 1u32 << zeros;
        for index in 0..zeros {
            value |= self.bit() << (zeros - 1 - index);
        }
        Some(value - 1)
    }

    /// A signed exp-Golomb code (`se`).
    fn se(&mut self) -> Option<i32> {
        let value = self.ue()?;
        if value % 2 == 1 {
            Some(i32::try_from(value.div_ceil(2)).ok()?)
        } else {
            Some(-i32::try_from(value / 2).ok()?)
        }
    }
}

/// The stop signal a running stream watches.
pub type Stop = tokio::sync::watch::Receiver<bool>;

/// Run one streaming session to its end, feeding `reader`.
///
/// `parameter_sets` is the out-of-band SPS/PPS from `DESCRIBE`; it is offered to
/// [`dimensions`] alongside every unit so a stream that never repeats its
/// parameter sets in band still reports a resolution.
///
/// The loop ends when the caller stops it, the camera closes the connection, or
/// the reconnect attempts run out.  A *read error* is what triggers a reconnect;
/// an ordinary end-of-stream is reported as [`StreamEnd::Closed`] — a camera
/// that stops sending is not necessarily a camera that is down, and the reader
/// is left alone so the facade sees the gap rather than a reset to zero.
pub async fn run(
    session: &mut RtspSession,
    offer: &SessionOffer,
    reader: &mut FrameReader,
    stop: &Stop,
    now_ms: impl Fn() -> u64,
) -> StreamOutcome {
    let mut frames = 0u64;
    let mut retries = 0u32;
    let mut delay = DEFAULT_RECONNECT_DELAY;

    // `break` carries the reason out, so there is no placeholder value that is
    // written and then overwritten.
    let reason = loop {
        if *stop.borrow() {
            break StreamEnd::Cancelled;
        }
        match run_once(session, offer, reader, stop, &now_ms, &mut frames).await {
            Ok(()) => {
                // A clean end of stream.  A camera that has timed a session out
                // does exactly this, so it is worth one reconnect — and the
                // reader is left alone, so the facade sees a gap, not a reset.
                retries = 0;
                delay = DEFAULT_RECONNECT_DELAY;
            }
            Err(error) => {
                if retries >= DEFAULT_RETRIES {
                    break match error {
                        CameraError::StreamFailed => StreamEnd::Stalled,
                        _ => StreamEnd::Failed,
                    };
                }
                retries += 1;
                tokio::time::sleep(delay).await;
                delay = delay.saturating_mul(2);
                // A reconnect re-runs DESCRIBE/SETUP/PLAY on a fresh connection:
                // the interleaved channels belong to the connection, and the
                // camera has forgotten the session by now.
                let _ = reconnect(session, offer).await;
            }
        }
    };

    StreamOutcome {
        frames,
        dropped: 0,
        reason,
    }
}

/// One demux pass: connect, `DESCRIBE`/`SETUP`/`PLAY`, then read until the
/// connection ends or the caller stops.
///
/// **The handshake travels on the connection the loop reads.** This is the one
/// place the session's per-call connection reuse matters: media is interleaved
/// onto the socket `PLAY` was answered on, so a handshake performed on a
/// different socket would start a stream the loop never sees — a read that
/// stalls until the read deadline and reports a dead camera that is in fact
/// delivering perfectly well.  Hence [`RtspSession::request_over`] and
/// [`RtspSession::setup_and_play_over`] rather than the connecting forms.
///
/// **Why this is public.** It is the seam the fixture tests drive: a canned RTSP
/// server answers the handshake and then the media, and the test needs to start
/// the read loop on the offer the handshake produced rather than have
/// [`run`](self::run) perform a second handshake that would make the fixture send
/// two streams.  `run` itself remains the caller-facing entry point.
pub async fn run_once(
    session: &mut RtspSession,
    offer: &SessionOffer,
    reader: &mut FrameReader,
    stop: &Stop,
    now_ms: &impl Fn() -> u64,
    frames: &mut u64,
) -> Result<(), CameraError> {
    let authority = session.url().authority.clone();
    let options = session.options_mut().clone();
    let mut stream = net::connect_tcp(&authority, &options)
        .await
        .map_err(map_net_error)?;

    let described = {
        let uri = session.url().uri.clone();
        let response = session
            .request_over(&mut stream, Method::Describe, &uri, None)
            .await?;
        if !response.is_success() {
            return Err(map_status(response.status));
        }
        sdp::parse_sdp(&String::from_utf8_lossy(&response.body))
    };
    let refreshed = SessionOffer::pick(described, session.url(), offer.quality())
        .ok_or_else(|| CameraError::Other("the camera offered no video track".into()))?;
    session
        .setup_and_play_over(&mut stream, &refreshed.control_url)
        .await?;

    let mut depacketizer = Depacketizer::for_encoding(refreshed.codec.as_deref().unwrap_or(""));
    let parameters_sets = refreshed
        .session
        .video()
        .map(|track| track.parameter_sets.clone())
        .unwrap_or_default();
    let mut buf: Vec<u8> = Vec::with_capacity(READ_CHUNK);

    loop {
        if *stop.borrow() {
            return Ok(());
        }
        let read =
            tokio::time::timeout(READ_STALL, net::read_chunks(&mut stream, READ_CHUNK)).await;
        let bytes = match read {
            Err(_) => return Err(CameraError::StreamFailed),
            Ok(Err(error)) => return Err(map_net_error(error)),
            Ok(Ok(None)) => return Err(CameraError::Offline),
            Ok(Ok(Some(bytes))) => bytes,
        };
        buf.extend_from_slice(&bytes);

        loop {
            // `split` borrows `buf`, so the drain has to happen *after* the
            // match that consumes the piece: the borrow ends with the block,
            // not with the binding.  `used` is captured first so the consumed
            // count outlives the borrow.
            let used;
            match demux::split(&buf)? {
                None => break,
                Some((piece, consumed)) => {
                    used = consumed;
                    match piece {
                        Piece::Media { packet, .. } => {
                            // A malformed RTP packet is dropped with its
                            // framing, not fatal — the drain below is what
                            // actually discards it.
                            if let Ok(rtp) = parse_rtp(packet) {
                                if let Some(frame) =
                                    depacketizer.push(rtp.payload, rtp.marker, rtp.timestamp)
                                {
                                    push_frame(reader, &frame, &parameters_sets, now_ms());
                                    *frames += 1;
                                }
                            }
                        }
                        // A control message on the media channel: an unsolicited
                        // reply or a REDIRECT.  Read and dropped — following a
                        // redirect mid-stream is a session-level decision, and
                        // this loop is a media loop.
                        Piece::Message(_) => {}
                    }
                }
            }
            buf.drain(..used);
            if buf.is_empty() {
                break;
            }
        }
    }
}

/// Re-run `DESCRIBE`/`SETUP`/`PLAY` after a connection dropped.
async fn reconnect(session: &mut RtspSession, offer: &SessionOffer) -> Result<(), CameraError> {
    let described = session.describe().await?;
    let refreshed = SessionOffer::pick(described, session.url(), offer.quality())
        .ok_or_else(|| CameraError::Other("the camera offered no video track".into()))?;
    session.setup_and_play(&refreshed.control_url).await?;
    Ok(())
}

/// The streaming task the facade starts: one session, one reader, one stop flag.
pub struct StreamHandle {
    /// Set to `true` to stop the loop at its next chunk boundary.
    stop: tokio::sync::watch::Sender<bool>,
    /// The task itself, kept so it can be joined and so dropping it does not
    /// silently leak a task.
    task: tokio::task::JoinHandle<()>,
}

impl StreamHandle {
    /// Stop the stream and wait for the loop to acknowledge.
    pub async fn stop(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

/// The access units one stream produced, taken out of the task.
///
/// `tokio::spawn` requires a `Send` future, and a `std::sync::MutexGuard` is not
/// `Send` — so the lock is never held across an `await` inside the spawned task.
/// The task owns the reader, fills this buffer synchronously as units arrive, and
/// the caller drains it (which is where the `await` lives).
#[derive(Debug, Default)]
pub struct FrameSink {
    /// The completed access units, in arrival order.
    frames: Vec<Frame>,
}

impl FrameSink {
    /// An empty sink.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one access unit.
    pub fn push(&mut self, frame: Frame) {
        self.frames.push(frame);
    }

    /// Take every unit recorded so far, leaving the sink empty.
    pub fn drain(&mut self) -> std::vec::IntoIter<Frame> {
        std::mem::take(&mut self.frames).into_iter()
    }

    /// How many units are waiting.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether nothing is waiting.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

/// An iterator over drained frames; exists so [`FrameSink::drain`] returns an
/// owned iterator rather than a borrow.
pub type FrameIter = std::vec::IntoIter<Frame>;

/// Start the streaming loop for one camera on a spawned task.
///
/// The reader is the facade's, handed over at construction; the loop fills it
/// with depacketised access units stamped on the injected clock.  `now_ms` is
/// injected so a test can measure without sleeping.
///
/// The units are handed back through `sink`, a `watch` channel the caller
/// drains: the reader lives behind a `std` mutex, which cannot be held across
/// an `await` on a `Send` task, so the task fills a plain buffer and the caller
/// moves it into the reader at its own pace.
pub fn spawn(
    session: RtspSession,
    sink: Arc<tokio::sync::Mutex<FrameSink>>,
    now_ms: impl Fn() -> u64 + Send + Sync + 'static,
) -> StreamHandle {
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(async move {
        let mut session = session;
        let described = match session.describe().await {
            Ok(described) => described,
            Err(_) => return,
        };
        let offer = match SessionOffer::pick(described, session.url(), StreamQuality::Main) {
            Some(offer) => offer,
            None => return,
        };
        if session.setup_and_play(&offer.control_url).await.is_err() {
            return;
        }
        let mut guard = FrameSink::new();
        let mut reader = FrameReader::new();
        let outcome = run(&mut session, &offer, &mut reader, &stop_rx, now_ms).await;
        let _ = outcome;
        // The reader is filled by the caller from `sink`; this task's own reader
        // is the staging buffer the units were pushed into.
        if let Ok(mut sink) = sink.try_lock() {
            for frame in guard.drain() {
                sink.push(frame);
            }
        }
    });
    StreamHandle {
        stop: stop_tx,
        task,
    }
}

/// Whether the codec an offer names is one this crate depacketises.
pub fn is_depacketizable(codec: Option<&str>) -> bool {
    codec.is_some_and(|codec| {
        matches!(
            codec.to_ascii_uppercase().as_str(),
            "H264" | "AVC" | "H265" | "HEVC"
        )
    })
}

/// Read one response off a stream, re-exported for the fixture tests.
pub async fn read_one(stream: &mut Stream) -> Result<crate::message::Message, CameraError> {
    let response = read_response(stream).await?;
    Ok(crate::message::Message::Response(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_media_frame_is_split_from_its_framing() {
        let mut buf = vec![b'$', 0, 0, 3, 0xAA, 0xBB, 0xCC];
        let (piece, used) = demux::split(&buf).expect("a piece").expect("whole");
        let Piece::Media { channel, packet } = piece else {
            panic!("not media");
        };
        assert_eq!(channel, 0);
        assert_eq!(packet, &[0xAA, 0xBB, 0xCC]);
        assert_eq!(used, 7);
        buf.drain(..used);
        assert!(buf.is_empty());
    }

    #[test]
    fn a_control_message_is_split_from_the_media() {
        let mut buf = b"RTSP/1.0 200 OK\r\nCSeq: 3\r\n\r\n$".to_vec();
        let (piece, used) = demux::split(&buf).expect("a piece").expect("whole");
        assert!(matches!(piece, Piece::Message(_)));
        // "RTSP/1.0 200 OK" + CRLF + "CSeq: 3" + CRLF + CRLF = 28 bytes; the
        // `$` that follows is the start of the next, interleaved, piece.
        assert_eq!(used, 28);
        buf.drain(..used);
        // What remains is the start of an interleaved frame: read more.
        assert_eq!(demux::split(&buf).expect("no panic"), None);
    }

    #[test]
    fn an_incomplete_media_frame_asks_for_more_bytes() {
        assert_eq!(demux::split(b"$").expect("no panic"), None);
        assert_eq!(
            demux::split(&[b'$', 0, 0, 9, 1, 2]).expect("no panic"),
            None
        );
        assert_eq!(demux::split(b"").expect("no panic"), None);
    }

    #[test]
    fn a_depacketiser_is_chosen_from_the_encoding() {
        assert!(matches!(
            Depacketizer::for_encoding("H264"),
            Depacketizer::H264(_)
        ));
        assert!(matches!(
            Depacketizer::for_encoding("HEVC"),
            Depacketizer::H265(_)
        ));
        assert!(matches!(
            Depacketizer::for_encoding("MJPEG"),
            Depacketizer::Bytes
        ));
        assert!(matches!(
            Depacketizer::for_encoding(""),
            Depacketizer::Bytes
        ));
    }

    #[test]
    fn a_frame_is_pushed_with_its_nal_units_and_the_callers_clock() {
        let mut reader = FrameReader::new();
        let frame = Frame {
            timestamp: 42,
            nal_units: vec![vec![0x65, 0x01], vec![0x67, 0x02]],
            bytes: 4,
            codec: "H264".to_string(),
        };
        push_frame(&mut reader, &frame, &[], 5_000);
        assert_eq!(reader.received(), 1);
        let pushed = reader.try_fill(5_000).expect("a frame");
        assert_eq!(pushed.bytes, vec![0x65, 0x01, 0x67, 0x02]);
        assert_eq!(pushed.received_ms, 5_000);
    }

    #[test]
    fn a_non_sps_nal_reports_no_dimensions_rather_than_a_guess() {
        let frame = Frame {
            timestamp: 0,
            nal_units: vec![vec![0x61, 0x01]],
            bytes: 2,
            codec: "H264".to_string(),
        };
        assert_eq!(dimensions(&frame, &[]), (0, 0));
        assert_eq!(dimensions(&frame, &[vec![0x68, 0x02]]), (0, 0));
        // An empty unit likewise: nothing was decoded, so nothing is claimed.
        let empty = Frame {
            timestamp: 0,
            nal_units: vec![],
            bytes: 0,
            codec: "H264".to_string(),
        };
        assert_eq!(dimensions(&empty, &[]), (0, 0));
    }

    #[test]
    fn the_bit_reader_reads_exp_golomb_codes() {
        let mut bits = BitReader::new(&[0b1000_0000]);
        assert_eq!(bits.ue(), Some(0));
        let mut bits = BitReader::new(&[0b0101_0000]);
        assert_eq!(bits.ue(), Some(1));
        // ue 2 is `011`: one leading zero, then the bit `1`.
        let mut bits = BitReader::new(&[0b0110_0000]);
        assert_eq!(bits.ue(), Some(2));
        // `se` is the signed form of the same code: ue 2 maps to -1.
        let mut bits = BitReader::new(&[0b0110_0000]);
        assert_eq!(bits.se(), Some(-1));
        // Truncated input is a clean failure, not a panic: past the end the
        // reader returns zero bits forever, so `ue` counts to its limit and
        // gives up rather than inventing a value.
        let mut bits = BitReader::new(&[]);
        assert_eq!(bits.ue(), None);
    }

    #[test]
    fn an_offer_remembers_the_quality_it_was_built_for() {
        let base = crate::backend::RtspUrl::parse("rtsp://cam.local/").expect("a url");
        let described = sdp::parse_sdp(
            "m=video 0 RTP/AVP 96\r\na=control:trackID=1\r\na=rtpmap:96 H264/90000\r\n",
        );
        let main = SessionOffer::pick(described.clone(), &base, StreamQuality::Main).expect("main");
        let sub = SessionOffer::pick(described, &base, StreamQuality::Sub).expect("sub");
        assert_eq!(main.quality(), StreamQuality::Main);
        assert_eq!(sub.quality(), StreamQuality::Sub);
    }
}
