//! GOP-cached NALU Multiplexer for sub-streams.
//!
//! Pulls an RTSP sub-stream (`sub_rtsp_path`) once using `cctv-rtp` and `cctv-transport`,
//! parses H.264 / H.265 NALU headers, caches the latest parameter sets and keyframe:
//! `[SPS, PPS, (VPS), IDR_Keyframe]`, and broadcasts NAL units to subscribers.
//!
//! When a Mobile client connects to `GET /agent/cameras/{id}/live_ws`, it immediately receives
//! the cached `[SPS, PPS, (VPS), IDR_Keyframe]` burst so the client's WASM/WebCodecs
//! decoder renders frame 1 with zero GOP wait, then subscribes the socket to the live
//! `tokio::sync::broadcast` NALU channel.

use std::sync::Arc;
use std::time::Duration;

use cctv_core::CameraError;
use cctv_rtp::h264::H264Depacketizer;
use cctv_rtp::h265::{parse_nal_header, H265Depacketizer};
use cctv_rtp::parse_rtp;
use cctv_rtsp::message::{parse_interleaved, Message, Method, Request};
use cctv_rtsp::sdp;
use cctv_transport::digest::{self, client_nonce, digest_authorization};
use cctv_transport::net::{self, Stream, TcpOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{broadcast, Mutex, RwLock};

/// Chunk size for reading media from the underlying socket.
pub const READ_CHUNK: usize = 65_536;

/// Codec type for the NAL multiplexer stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    /// H.264 / AVC video.
    H264,
    /// H.265 / HEVC video.
    H265,
}

/// NAL unit classification for caching and decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NaluType {
    /// Video Parameter Set (H.265 only).
    Vps,
    /// Sequence Parameter Set (H.264 / H.265).
    Sps,
    /// Picture Parameter Set (H.264 / H.265).
    Pps,
    /// Instantaneous Decoder Refresh / Keyframe slice.
    IdrKeyframe,
    /// Non-IDR / regular video slice or other NAL unit.
    Other,
}

/// One raw NAL unit with its codec classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nalu {
    /// Codec standard.
    pub codec: VideoCodec,
    /// Classified NAL type.
    pub nalu_type: NaluType,
    /// Raw NAL unit payload (including NAL header, without start codes).
    pub data: Vec<u8>,
}

impl Nalu {
    /// Create a new Nalu by inspecting the NAL unit payload bytes.
    pub fn parse(codec: VideoCodec, data: Vec<u8>) -> Self {
        let nalu_type = classify_nalu(codec, &data);
        Self {
            codec,
            nalu_type,
            data,
        }
    }
}

/// Classify a NAL unit's type from its header bytes.
pub fn classify_nalu(codec: VideoCodec, data: &[u8]) -> NaluType {
    if data.is_empty() {
        return NaluType::Other;
    }
    match codec {
        VideoCodec::H264 => {
            // H.264: 1-byte header: forbidden_zero_bit (1), nal_ref_idc (2), nal_unit_type (5)
            match data[0] & 0x1F {
                7 => NaluType::Sps,
                8 => NaluType::Pps,
                5 => NaluType::IdrKeyframe,
                _ => NaluType::Other,
            }
        }
        VideoCodec::H265 => {
            // H.265: 2-byte header: (buf[0] >> 1) & 0x3F
            if let Some(header) = parse_nal_header(data) {
                match header.nal_type {
                    32 => NaluType::Vps,
                    33 => NaluType::Sps,
                    34 => NaluType::Pps,
                    19 | 20 => NaluType::IdrKeyframe, // IDR_W_RADL, IDR_N_LP
                    _ => NaluType::Other,
                }
            } else {
                NaluType::Other
            }
        }
    }
}

/// GOP cache containing the latest parameter sets and keyframe burst.
///
/// Stores `[SPS, PPS, (VPS), IDR_Keyframe]` in wire order so connected clients
/// can immediately start decoding with zero GOP wait.
#[derive(Debug, Default, Clone)]
pub struct GopCache {
    /// Latest Video Parameter Set (H.265).
    pub vps: Option<Vec<u8>>,
    /// Latest Sequence Parameter Set.
    pub sps: Option<Vec<u8>>,
    /// Latest Picture Parameter Set.
    pub pps: Option<Vec<u8>>,
    /// Latest IDR Keyframe NAL units.
    pub idr_keyframe: Vec<Vec<u8>>,
}

impl GopCache {
    /// Create an empty GOP cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Update cache with a NAL unit. If a new IDR keyframe arrives, clears
    /// previous IDR slices and starts accumulating the current keyframe.
    pub fn update(&mut self, nalu: &Nalu) {
        match nalu.nalu_type {
            NaluType::Vps => {
                self.vps = Some(nalu.data.clone());
            }
            NaluType::Sps => {
                self.sps = Some(nalu.data.clone());
            }
            NaluType::Pps => {
                self.pps = Some(nalu.data.clone());
            }
            NaluType::IdrKeyframe => {
                self.idr_keyframe.push(nalu.data.clone());
            }
            NaluType::Other => {}
        }
    }

    /// Reset IDR keyframe slices (e.g. at the start of a new keyframe access unit).
    pub fn clear_idr(&mut self) {
        self.idr_keyframe.clear();
    }

    /// Build the zero-GOP burst: `[SPS, PPS, (VPS), IDR_Keyframe]`.
    ///
    /// The burst contains all parameter sets needed by WASM/WebCodecs decoders,
    /// followed by the IDR keyframe slices.
    pub fn initial_burst(&self, codec: VideoCodec) -> Vec<Nalu> {
        let mut burst = Vec::new();
        match codec {
            VideoCodec::H265 => {
                if let Some(vps) = &self.vps {
                    burst.push(Nalu {
                        codec,
                        nalu_type: NaluType::Vps,
                        data: vps.clone(),
                    });
                }
                if let Some(sps) = &self.sps {
                    burst.push(Nalu {
                        codec,
                        nalu_type: NaluType::Sps,
                        data: sps.clone(),
                    });
                }
                if let Some(pps) = &self.pps {
                    burst.push(Nalu {
                        codec,
                        nalu_type: NaluType::Pps,
                        data: pps.clone(),
                    });
                }
            }
            VideoCodec::H264 => {
                if let Some(sps) = &self.sps {
                    burst.push(Nalu {
                        codec,
                        nalu_type: NaluType::Sps,
                        data: sps.clone(),
                    });
                }
                if let Some(pps) = &self.pps {
                    burst.push(Nalu {
                        codec,
                        nalu_type: NaluType::Pps,
                        data: pps.clone(),
                    });
                }
            }
        }
        for slice in &self.idr_keyframe {
            burst.push(Nalu {
                codec,
                nalu_type: NaluType::IdrKeyframe,
                data: slice.clone(),
            });
        }
        burst
    }

    /// Returns `true` if the cache has sufficient information to decode
    /// (at least SPS, PPS, and at least one IDR keyframe slice).
    pub fn has_keyframe(&self) -> bool {
        self.sps.is_some() && self.pps.is_some() && !self.idr_keyframe.is_empty()
    }
}

/// A client subscription to the multiplexer stream.
pub struct StreamSubscription {
    /// Initial burst of `[SPS, PPS, (VPS), IDR_Keyframe]` for zero GOP wait.
    pub burst: Vec<Nalu>,
    /// Live NAL unit broadcast receiver.
    pub receiver: broadcast::Receiver<Nalu>,
}

/// Configuration for the sub-stream NAL multiplexer.
#[derive(Debug, Clone)]
pub struct StreamMuxConfig {
    /// Upstream RTSP sub-stream URL (`sub_rtsp_path`).
    pub sub_rtsp_path: String,
    /// Channel capacity for broadcast channel (default: 256).
    pub broadcast_capacity: usize,
    /// Credentials for RTSP authentication.
    pub username: Option<String>,
    /// Password for RTSP authentication.
    pub password: Option<String>,
}

impl StreamMuxConfig {
    /// Create a new configuration for the given `sub_rtsp_path`.
    pub fn new(sub_rtsp_path: impl Into<String>) -> Self {
        Self {
            sub_rtsp_path: sub_rtsp_path.into(),
            broadcast_capacity: 256,
            username: None,
            password: None,
        }
    }

    /// Builder: set username and password.
    pub fn with_credentials(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.username = Some(username.into());
        self.password = Some(password.into());
        self
    }
}

/// Single upstream RTSP pull multiplexer that distributes NAL units to multiple
/// clients with GOP caching.
pub struct StreamMux {
    config: StreamMuxConfig,
    gop_cache: Arc<RwLock<GopCache>>,
    sender: broadcast::Sender<Nalu>,
    codec: Arc<RwLock<VideoCodec>>,
    running: Arc<Mutex<Option<tokio::sync::watch::Sender<bool>>>>,
}

impl StreamMux {
    /// Create a new `StreamMux` for a camera's sub-stream.
    pub fn new(config: StreamMuxConfig) -> Self {
        let (sender, _) = broadcast::channel(config.broadcast_capacity);
        Self {
            config,
            gop_cache: Arc::new(RwLock::new(GopCache::new())),
            sender,
            codec: Arc::new(RwLock::new(VideoCodec::H264)),
            running: Arc::new(Mutex::new(None)),
        }
    }

    /// Read the detected or configured video codec.
    pub async fn codec(&self) -> VideoCodec {
        *self.codec.read().await
    }

    /// Direct access to the current GOP cache snapshot.
    pub async fn gop_cache(&self) -> GopCache {
        self.gop_cache.read().await.clone()
    }

    /// Subscribe a client (such as for `GET /agent/cameras/{id}/live_ws`).
    ///
    /// Returns the cached `[SPS, PPS, (VPS), IDR_Keyframe]` burst followed by
    /// a subscription receiver to the live broadcast channel.
    pub async fn subscribe(&self) -> StreamSubscription {
        let receiver = self.sender.subscribe();
        let codec = *self.codec.read().await;
        let cache = self.gop_cache.read().await;
        let burst = cache.initial_burst(codec);
        StreamSubscription { burst, receiver }
    }

    /// Start the upstream pulling task if not already started.
    pub async fn start(&self) -> Result<(), CameraError> {
        let mut guard = self.running.lock().await;
        if guard.is_some() {
            return Ok(());
        }

        let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
        *guard = Some(stop_tx);

        let config = self.config.clone();
        let gop_cache = self.gop_cache.clone();
        let sender = self.sender.clone();
        let codec_lock = self.codec.clone();

        tokio::spawn(async move {
            run_mux_loop(config, gop_cache, sender, codec_lock, stop_rx).await;
        });

        Ok(())
    }

    /// Stop the upstream pulling task.
    pub async fn stop(&self) {
        let mut guard = self.running.lock().await;
        if let Some(stop_tx) = guard.take() {
            let _ = stop_tx.send(true);
        }
    }

    /// Ingest a completed access unit into the GOP cache and broadcast channel.
    /// Useful for internal processing, simulated streams, or test mocking.
    pub async fn ingest_access_unit(&self, codec: VideoCodec, nal_units: Vec<Vec<u8>>) {
        let mut cache = self.gop_cache.write().await;
        let mut is_idr_frame = false;
        let parsed_nals: Vec<Nalu> = nal_units
            .into_iter()
            .map(|data| {
                let nalu = Nalu::parse(codec, data);
                if nalu.nalu_type == NaluType::IdrKeyframe {
                    is_idr_frame = true;
                }
                nalu
            })
            .collect();

        if is_idr_frame {
            cache.clear_idr();
        }

        for nalu in parsed_nals {
            cache.update(&nalu);
            let _ = self.sender.send(nalu);
        }
    }
}

/// Upstream RTSP demux loop for `StreamMux`.
async fn run_mux_loop(
    config: StreamMuxConfig,
    gop_cache: Arc<RwLock<GopCache>>,
    sender: broadcast::Sender<Nalu>,
    codec_lock: Arc<RwLock<VideoCodec>>,
    stop_rx: tokio::sync::watch::Receiver<bool>,
) {
    let (authority, host, path, is_tls) = match parse_rtsp_url(&config.sub_rtsp_path) {
        Some(parsed) => parsed,
        None => return,
    };

    let options = TcpOptions::default();

    loop {
        if *stop_rx.borrow() {
            break;
        }

        let mut stream = if is_tls {
            match net::connect_tls(&authority, &host, &options).await {
                Ok(s) => s,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                    continue;
                }
            }
        } else {
            match net::connect_tcp(&authority, &options).await {
                Ok(s) => s,
                Err(_) => {
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                    continue;
                }
            }
        };

        // DESCRIBE -> SETUP -> PLAY handshake over TCP
        let sdp_result = match rtsp_describe(
            &mut stream,
            &config.sub_rtsp_path,
            &path,
            config.username.as_deref(),
            config.password.as_deref(),
        )
        .await
        {
            Ok(s) => s,
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(1000)).await;
                continue;
            }
        };

        let session_sdp = sdp::parse_sdp(&sdp_result);
        let Some(video_track) = session_sdp.video() else {
            tokio::time::sleep(Duration::from_millis(1000)).await;
            continue;
        };

        let codec = if video_track.encoding.eq_ignore_ascii_case("H265")
            || video_track.encoding.eq_ignore_ascii_case("HEVC")
        {
            VideoCodec::H265
        } else {
            VideoCodec::H264
        };

        {
            let mut c = codec_lock.write().await;
            *c = codec;
        }

        // Cache out-of-band parameter sets from SDP if present
        {
            let mut cache = gop_cache.write().await;
            for ps in &video_track.parameter_sets {
                let nalu = Nalu::parse(codec, ps.clone());
                cache.update(&nalu);
            }
        }

        let control_url = if video_track.control.starts_with("rtsp://")
            || video_track.control.starts_with("rtsps://")
        {
            video_track.control.clone()
        } else {
            format!("{}/{}", config.sub_rtsp_path.trim_end_matches('/'), video_track.control.trim_start_matches('/'))
        };

        let session_id = match rtsp_setup_and_play(
            &mut stream,
            &control_url,
            &config.sub_rtsp_path,
            config.username.as_deref(),
            config.password.as_deref(),
        )
        .await
        {
            Ok(sid) => sid,
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(1000)).await;
                continue;
            }
        };

        let _ = session_id;

        // Start RTP depacketiser and demux loop
        let mut h264_depack = H264Depacketizer::new();
        let mut h265_depack = H265Depacketizer::new();
        let mut buf = Vec::with_capacity(READ_CHUNK);

        loop {
            if *stop_rx.borrow() {
                return;
            }

            let read =
                tokio::time::timeout(Duration::from_secs(15), net::read_chunks(&mut stream, READ_CHUNK)).await;
            let bytes = match read {
                Ok(Ok(Some(bytes))) => bytes,
                _ => break, // Reconnect on stall or EOF
            };

            buf.extend_from_slice(&bytes);

            while !buf.is_empty() {
                if buf[0] == b'$' {
                    match parse_interleaved(&buf) {
                        Some(Ok((frame, used))) => {
                            if let Ok(rtp) = parse_rtp(frame.payload) {
                                let access_unit = match codec {
                                    VideoCodec::H264 => {
                                        h264_depack.push(rtp.payload, rtp.marker, rtp.timestamp)
                                    }
                                    VideoCodec::H265 => {
                                        h265_depack.push(rtp.payload, rtp.marker, rtp.timestamp)
                                    }
                                };

                                if let Some(unit) = access_unit {
                                    let mut is_idr = false;
                                    let parsed_nals: Vec<Nalu> = unit
                                        .nal_units
                                        .into_iter()
                                        .map(|data| {
                                            let n = Nalu::parse(codec, data);
                                            if n.nalu_type == NaluType::IdrKeyframe {
                                                is_idr = true;
                                            }
                                            n
                                        })
                                        .collect();

                                    {
                                        let mut cache = gop_cache.write().await;
                                        if is_idr {
                                            cache.clear_idr();
                                        }
                                        for nalu in &parsed_nals {
                                            cache.update(nalu);
                                        }
                                    }

                                    for nalu in parsed_nals {
                                        let _ = sender.send(nalu);
                                    }
                                }
                            }
                            buf.drain(..used);
                        }
                        Some(Err(_)) => {
                            buf.drain(..1);
                        }
                        None => break, // need more bytes
                    }
                } else {
                    // RTSP control message or status response
                    match Message::parse(&buf) {
                        Ok((_msg, used)) => {
                            buf.drain(..used);
                        }
                        Err(cctv_rtsp::error::RtspError::Incomplete) => break,
                        Err(_) => {
                            buf.drain(..1);
                        }
                    }
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Helper to parse RTSP URL into `(authority, host, path, is_tls)`.
fn parse_rtsp_url(url: &str) -> Option<(String, String, String, bool)> {
    let (scheme, rest) = url.split_once("://")?;
    let is_tls = scheme.eq_ignore_ascii_case("rtsps");
    let (auth, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    let authority = match auth.rfind('@') {
        Some(idx) => &auth[idx + 1..],
        None => auth,
    };
    let host = match authority.find(':') {
        Some(idx) => &authority[..idx],
        None => authority,
    };
    let authority_with_port = if authority.contains(':') {
        authority.to_string()
    } else {
        format!("{authority}:554")
    };
    Some((authority_with_port, host.to_string(), path.to_string(), is_tls))
}

/// Send RTSP DESCRIBE with Digest / Basic negotiation and return SDP string.
async fn rtsp_describe(
    stream: &mut Stream,
    url: &str,
    path: &str,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<String, CameraError> {
    let req = Request::new(Method::Describe, url, 1);
    let bytes = req.to_bytes();
    stream.write_all(&bytes).await.map_err(|e| CameraError::Other(e.to_string()))?;

    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream.read(&mut chunk).await.map_err(|e| CameraError::Other(e.to_string()))?;
        if n == 0 {
            return Err(CameraError::Offline);
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Ok((Message::Response(resp), _)) = Message::parse(&buf) {
            if let (401, Some(user)) = (resp.status, username) {
                let pass = password.unwrap_or("");
                let challenge_hdr = resp.headers.get("WWW-Authenticate");
                if let Some(ch) = challenge_hdr.and_then(digest::parse_challenge) {
                    let auth_val = match ch {
                        digest::Challenge::Digest(d) => digest_authorization(
                            &d,
                            "DESCRIBE",
                            path,
                            user,
                            pass,
                            &client_nonce(),
                            1,
                        ),
                        digest::Challenge::Basic => digest::basic_authorization(user, pass),
                    };
                    let mut req2 = Request::new(Method::Describe, url, 2);
                    req2.headers.insert("Authorization", auth_val);
                    let b2 = req2.to_bytes();
                    stream.write_all(&b2).await.map_err(|e| CameraError::Other(e.to_string()))?;

                    let mut buf2 = Vec::with_capacity(4096);
                    loop {
                        let n2 = stream.read(&mut chunk).await.map_err(|e| CameraError::Other(e.to_string()))?;
                        if n2 == 0 {
                            return Err(CameraError::Offline);
                        }
                        buf2.extend_from_slice(&chunk[..n2]);
                        if let Ok((Message::Response(resp2), _)) = Message::parse(&buf2) {
                            if resp2.is_success() {
                                return Ok(String::from_utf8_lossy(&resp2.body).to_string());
                            }
                            return Err(CameraError::Other(format!("RTSP status {}", resp2.status)));
                        }
                    }
                }
            }
            if resp.is_success() {
                return Ok(String::from_utf8_lossy(&resp.body).to_string());
            }
            return Err(CameraError::Other(format!("RTSP status {}", resp.status)));
        }
    }
}

/// Send RTSP SETUP and PLAY requests.
async fn rtsp_setup_and_play(
    stream: &mut Stream,
    control_url: &str,
    base_url: &str,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<String, CameraError> {
    let _ = (base_url, username, password);
    let mut setup_req = Request::new(Method::Setup, control_url, 3);
    setup_req.headers.insert("Transport", "RTP/AVP/TCP;unicast;interleaved=0-1");
    let bytes = setup_req.to_bytes();
    stream.write_all(&bytes).await.map_err(|e| CameraError::Other(e.to_string()))?;

    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 1024];
    let session_id = loop {
        let n = stream.read(&mut chunk).await.map_err(|e| CameraError::Other(e.to_string()))?;
        if n == 0 {
            return Err(CameraError::Offline);
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Ok((Message::Response(resp), _)) = Message::parse(&buf) {
            if resp.is_success() {
                let sid = resp.headers.get("Session").unwrap_or("").to_string();
                let clean_sid = sid.split(';').next().unwrap_or("").to_string();
                break clean_sid;
            }
            return Err(CameraError::Other(format!("SETUP failed: {}", resp.status)));
        }
    };

    let mut play_req = Request::new(Method::Play, control_url, 4);
    if !session_id.is_empty() {
        play_req.headers.insert("Session", &session_id);
    }
    let bytes_play = play_req.to_bytes();
    stream.write_all(&bytes_play).await.map_err(|e| CameraError::Other(e.to_string()))?;

    Ok(session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h264_nalu_classification() {
        assert_eq!(classify_nalu(VideoCodec::H264, &[0x67, 0x42]), NaluType::Sps);
        assert_eq!(classify_nalu(VideoCodec::H264, &[0x68, 0xCE]), NaluType::Pps);
        assert_eq!(
            classify_nalu(VideoCodec::H264, &[0x65, 0x88]),
            NaluType::IdrKeyframe
        );
        assert_eq!(
            classify_nalu(VideoCodec::H264, &[0x41, 0x9A]),
            NaluType::Other
        );
    }

    #[test]
    fn h265_nalu_classification() {
        // VPS: nal_type 32 -> (32 << 1) = 64 = 0x40
        assert_eq!(
            classify_nalu(VideoCodec::H265, &[0x40, 0x01]),
            NaluType::Vps
        );
        // SPS: nal_type 33 -> (33 << 1) = 66 = 0x42
        assert_eq!(
            classify_nalu(VideoCodec::H265, &[0x42, 0x01]),
            NaluType::Sps
        );
        // PPS: nal_type 34 -> (34 << 1) = 68 = 0x44
        assert_eq!(
            classify_nalu(VideoCodec::H265, &[0x44, 0x01]),
            NaluType::Pps
        );
        // IDR: nal_type 19 -> (19 << 1) = 38 = 0x26
        assert_eq!(
            classify_nalu(VideoCodec::H265, &[0x26, 0x01]),
            NaluType::IdrKeyframe
        );
    }

    #[tokio::test]
    async fn gop_cache_accumulates_and_bursts_zero_wait() {
        let mux = StreamMux::new(StreamMuxConfig::new("rtsp://localhost:8554/sub"));

        // Feed SPS and PPS
        mux.ingest_access_unit(
            VideoCodec::H264,
            vec![vec![0x67, 0x42, 0x00], vec![0x68, 0xCE]],
        )
        .await;

        // Feed IDR Keyframe
        mux.ingest_access_unit(VideoCodec::H264, vec![vec![0x65, 0x88, 0xAA, 0xBB]])
            .await;

        // Client connects to /live_ws
        let mut sub = mux.subscribe().await;
        assert_eq!(sub.burst.len(), 3);
        assert_eq!(sub.burst[0].nalu_type, NaluType::Sps);
        assert_eq!(sub.burst[1].nalu_type, NaluType::Pps);
        assert_eq!(sub.burst[2].nalu_type, NaluType::IdrKeyframe);

        // Feed subsequent P-frame (Other)
        mux.ingest_access_unit(VideoCodec::H264, vec![vec![0x41, 0x9A, 0xCC]])
            .await;

        let live_nalu = sub.receiver.recv().await.expect("live nalu");
        assert_eq!(live_nalu.nalu_type, NaluType::Other);
        assert_eq!(live_nalu.data, vec![0x41, 0x9A, 0xCC]);
    }

    #[test]
    fn rtsp_url_parser_handles_standard_urls() {
        let (auth, host, path, is_tls) = parse_rtsp_url("rtsp://admin:pass@10.0.0.1:8554/stream1").expect("parsed");
        assert_eq!(auth, "10.0.0.1:8554");
        assert_eq!(host, "10.0.0.1");
        assert_eq!(path, "/stream1");
        assert!(!is_tls);

        let (auth2, host2, path2, is_tls2) = parse_rtsp_url("rtsps://cam.local/live").expect("parsed");
        assert_eq!(auth2, "cam.local:554");
        assert_eq!(host2, "cam.local");
        assert_eq!(path2, "/live");
        assert!(is_tls2);
    }
}
