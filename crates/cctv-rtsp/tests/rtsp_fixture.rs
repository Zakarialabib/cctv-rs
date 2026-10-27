//! The RTSP fixture server, and the real-path tests that run the driver against it.
//!
//! **Why a server in `cctv-rtsp`'s own tests.** The parsers in
//! [`cctv_rtsp::message`] and [`cctv_rtsp::sdp`] are proven on hand-written byte
//! slices; what they cannot prove is that the *driver* speaks the protocol well
//! enough to be answered.  A canned RTSP server is the only way to prove
//! DESCRIBE → SETUP → PLAY → interleaved media end to end without a camera, a
//! network, or a credential — the fixture shape the plan's W3 gate names.
//!
//! **Why one connection per request.** The driver's [`RtspSession`] opens a fresh
//! TCP connection for every call, and its streaming loop opens one more for the
//! media.  A fixture that accepted a single connection would answer the first
//! request and hang up on the second, so the listener accepts as many times as
//! the session needs and serves each independently.
//!
//! **Why the media starts on `PLAY`.** Interleaved RTP is multiplexed onto the
//! very connection `PLAY` was answered on, so the media flag and its budget
//! belong to the connection rather than to the fixture.  That is also what makes
//! the budget per-connection: the handshake connections send nothing, and the
//! media connection sends exactly [`ACCESS_UNITS`] frames before closing, which
//! is how the driver's read loop ends on an ordinary end of stream.
//!
//! **Why nothing here depends on wall-clock time for its assertions.** The frames
//! are stamped by the test's injected clock, because the read loop takes `now_ms`
//! as a parameter, so a measured rate is exact whatever the machine's load.  The
//! pacing is tens of milliseconds — short enough that the suite runs in about a
//! second, long enough that a TCP segment boundary is not the thing under test.

#![cfg(feature = "net")]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use base64::engine::general_purpose;
use base64::Engine as _;
use cctv_core::{CameraError, StreamQuality};
use cctv_rtsp::backend::{RtspSession, RtspUrl, SessionOffer};
use cctv_rtsp::sdp;
use cctv_rtsp::streaming::{self, StreamEnd, StreamOutcome};
use cctv_transport::digest::{self, DigestChallenge};
use cctv_transport::reader::FrameReader;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, TcpStream};

/// The session id every `SETUP` answers with.
pub const SESSION_ID: &str = "12345678";
/// The realm the Digest variant challenges against.
pub const REALM: &str = "cctv-rs-fixture";
/// The nonce the Digest variant issues.  Fixed rather than random so the
/// expected `response` digest is computable in the test.
pub const NONCE: &str = "0f4ac1c0b6e94a2f8d7e6c5b4a392817";
/// The opaque value the Digest variant echoes back.
pub const OPAQUE: &str = "5ccc069c403ebaf9f0171e9517f40e41";
/// The username the Digest variant accepts.
pub const USERNAME: &str = "admin";
/// The password the Digest variant accepts.
pub const PASSWORD: &str = "hunter2";
/// The number of access units the media connection sends before closing.
pub const ACCESS_UNITS: u32 = 30;
/// The RTP timestamp clock rate, from the `a=rtpmap` line.
pub const CLOCK_RATE: u32 = 90_000;
/// How far apart, in RTP clock ticks, two consecutive access units are.
pub const TICKS_PER_UNIT: u32 = CLOCK_RATE / ACCESS_UNITS;
/// The inter-packet delay the server paces media with.
pub const PACE: std::time::Duration = std::time::Duration::from_millis(20);
/// The RTP payload type the SDP advertises.
pub const PAYLOAD_TYPE: u8 = 96;
/// The NAL header byte of an IDR slice (forbidden zero, no nal_ref_idc, type 5).
pub const NAL_HEADER: u8 = 0x65;
/// The channel the media is interleaved on.
pub const MEDIA_CHANNEL: u8 = 0;
/// The base64 SPS of a baseline 320x240 stream.
pub const SPS_BASE64: &str = "Z2QAH6wrwJUB42LuxEBuwB0lPA+L6bLz4S8k3g==";
/// The base64 PPS that goes with it.
pub const PPS_BASE64: &str = "aM48sA==";

/// The SDP body the `DESCRIBE` answers with.
///
/// The shape is exactly what [`sdp::parse_sdp`] reads: a session-level `s=`,
/// then one `m=` section whose `a=control` is relative (so the driver resolves
/// it against the session URL) and whose `a=fmtp` carries both
/// `packetization-mode` and `sprop-parameter-sets`.
pub fn sdp_body() -> String {
    format!(
        "v=0\r\n\
         o=- 1234567890 1 IN IP4 127.0.0.1\r\n\
         s=Fixture Camera\r\n\
         i=RTSP fixture stream\r\n\
         t=0 0\r\n\
         a=control:*\r\n\
         m=video 0 RTP/AVP {PAYLOAD_TYPE}\r\n\
         a=control:trackID=1\r\n\
         a=rtpmap:{PAYLOAD_TYPE} H264/{CLOCK_RATE}\r\n\
         a=fmtp:{PAYLOAD_TYPE} packetization-mode=1;profile-level-id=4d002a;\
         sprop-parameter-sets={SPS_BASE64},{PPS_BASE64}\r\n"
    )
}

/// The Digest challenge the authenticated variant issues.
pub fn challenge_header() -> String {
    format!("Digest realm=\"{REALM}\", nonce=\"{NONCE}\", opaque=\"{OPAQUE}\", qop=\"auth\"")
}

/// The SPS, decoded.
///
/// A function rather than a constant because decoding is the only thing that can
/// fail here, and it is asked for once per access unit.
pub fn sps() -> Vec<u8> {
    general_purpose::STANDARD
        .decode(SPS_BASE64)
        .expect("a valid SPS")
}

/// The PPS, decoded.
pub fn pps() -> Vec<u8> {
    general_purpose::STANDARD
        .decode(PPS_BASE64)
        .expect("a valid PPS")
}

/// One RTP packet: a 12-byte header followed by one single-NAL H.264 payload.
///
/// Hand-built rather than taken from `cctv-rtp` because `cctv-rtp` *parses* and
/// never serialises, and the fixture needs the opposite.  The header is the one
/// the depacketiser checks: version 2, no padding, no extension, no CSRC, the
/// payload type from the SDP, and the marker bit set on the last packet of the
/// access unit — which, for a single-NAL unit, is every packet.
pub fn rtp_packet(sequence: u16, timestamp: u32, payload: &[u8], marker: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + payload.len());
    // Version 2, no padding, no extension, zero CSRCs.
    out.push(0x80);
    // Marker, then the payload type.
    out.push(if marker { 0x80 } else { 0x00 } | (PAYLOAD_TYPE & 0x7F));
    out.extend_from_slice(&sequence.to_be_bytes());
    out.extend_from_slice(&timestamp.to_be_bytes());
    // A fixed SSRC: a fixture has no reason to vary it, and a stable one keeps
    // the packets of a session clearly one stream.
    out.extend_from_slice(&0x1234_5678u32.to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// One interleaved frame: `$`, the channel, the big-endian length, the packet.
pub fn interleaved(channel: u8, packet: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + packet.len());
    out.push(b'$');
    out.push(channel);
    out.extend_from_slice(&(packet.len() as u16).to_be_bytes());
    out.extend_from_slice(packet);
    out
}

/// The H.264 payload of one access unit: the parameter sets, then an IDR slice.
///
/// `sprop-parameter-sets` is out of band, but a decoder still needs the SPS and
/// PPS in the stream to decode the first keyframe, so the fixture repeats them
/// ahead of every IDR.  That is what a real camera does.
pub fn access_unit_payload(index: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(sps().len() + pps().len() + 4);
    out.extend_from_slice(&sps());
    out.extend_from_slice(&pps());
    // A slice body: the NAL header, then a counter so each unit is distinct.
    out.push(NAL_HEADER);
    out.extend_from_slice(&(index as u16).to_be_bytes());
    out
}

/// The clock the facade measures on: Unix milliseconds.
///
/// Shared by the fixture and the tests so a measured rate and the window it is
/// measured over come from one clock.
pub fn clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// The running fixture server.
pub struct FixtureServer {
    /// Where it listens.
    addr: SocketAddr,
    /// Whether `DESCRIBE` answers `401` before it answers.
    digest: bool,
    /// Every request received, in arrival order.
    shared: Arc<Shared>,
    /// The accept task; aborted on drop so a test never leaks a listener.
    task: tokio::task::JoinHandle<()>,
}

struct Shared {
    requests: std::sync::Mutex<Vec<Recorded>>,
}

/// One request the fixture received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// The method token.
    pub method: String,
    /// The request URI.
    pub uri: String,
    /// The request body, when it had one.
    pub body: Vec<u8>,
    /// Every header, names as sent.
    pub headers: Vec<(String, String)>,
}

impl Recorded {
    /// First header with this (case-insensitive) name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

impl FixtureServer {
    /// Serve on an ephemeral `127.0.0.1` port, with or without a Digest gate.
    pub async fn start(digest: bool) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared {
            requests: std::sync::Mutex::new(Vec::new()),
        });
        let s = Arc::clone(&shared);
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let s = Arc::clone(&s);
                tokio::spawn(async move {
                    let _ = serve(stream, &s, digest).await;
                });
            }
        });
        Ok(Self {
            addr,
            digest,
            shared,
            task,
        })
    }

    /// `rtsp://127.0.0.1:<port>/Streaming/Channels/101`.
    pub fn url(&self) -> RtspUrl {
        RtspUrl::parse(&format!("rtsp://{}/Streaming/Channels/101", self.addr)).expect("a url")
    }

    /// `rtsp://user:pass@127.0.0.1:<port>/Streaming/Channels/101`.
    pub fn url_with_credentials(&self) -> RtspUrl {
        RtspUrl::parse(&format!(
            "rtsp://{USERNAME}:{PASSWORD}@{}/Streaming/Channels/101",
            self.addr
        ))
        .expect("a url")
    }

    /// Every request received so far, in arrival order.
    pub fn requests(&self) -> Vec<Recorded> {
        self.shared
            .requests
            .lock()
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    /// Whether this fixture gates its requests behind a Digest challenge.
    pub fn digest(&self) -> bool {
        self.digest
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Whether an `Authorization` header answers this challenge correctly.
///
/// This is the same computation the driver performs, checked from the server's
/// side: it exists so the fixture can refuse a wrong answer rather than accept
/// any header that happens to start with `Digest `.  A fixture that accepted
/// anything would prove nothing about the driver's retry.
pub fn authorized(request: &Recorded, method: &str) -> bool {
    let Some(header) = request.header("Authorization") else {
        return false;
    };
    let Some(rest) = header.strip_prefix("Digest ") else {
        return false;
    };
    let mut fields = std::collections::HashMap::new();
    for pair in rest.split(',') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        fields.insert(
            key.trim().to_string(),
            value.trim().trim_matches('"').to_string(),
        );
    }
    if fields.get("username").map(String::as_str) != Some(USERNAME) {
        return false;
    }
    if fields.get("realm").map(String::as_str) != Some(REALM) {
        return false;
    }
    if fields.get("nonce").map(String::as_str) != Some(NONCE) {
        return false;
    }
    let Some(uri) = fields.get("uri") else {
        return false;
    };
    let expected = expected_response(
        method,
        uri,
        1,
        fields.get("cnonce").map_or("", String::as_str),
    );
    fields.get("response").map(String::as_str) == Some(expected.as_str())
}

/// The `response` digest the server expects for this request.
pub fn expected_response(method: &str, uri: &str, nonce_count: u32, cnonce: &str) -> String {
    let challenge = DigestChallenge {
        realm: REALM.to_string(),
        nonce: NONCE.to_string(),
        opaque: Some(OPAQUE.to_string()),
        algorithm: None,
        qop_auth: true,
    };
    let header = digest::digest_authorization(
        &challenge,
        method,
        uri,
        USERNAME,
        PASSWORD,
        cnonce,
        nonce_count,
    );
    header
        .split("response=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or_default()
        .to_string()
}

/// Serve one connection: the handshake, then the interleaved media.
///
/// **Why two tasks.** The server owes the client media from the moment `PLAY` is
/// answered and owes it *while* the client is still sending requests.  A single
/// loop that writes a frame and then reads cannot do both: the write blocks once
/// the client's receive window fills, and the client — which is itself waiting
/// for bytes — never drains, so the pair deadlocks.  Splitting the connection
/// into a writer and a reader is what makes the media a stream rather than a
/// stall.  Both halves share the media budget, so the requests cannot extend it.
async fn serve(stream: TcpStream, shared: &Shared, digest: bool) -> std::io::Result<()> {
    let (mut read_half, write_half) = stream.into_split();
    let budget = Arc::new(AtomicU32::new(0));
    let playing = Arc::new(AtomicBool::new(false));

    // A response and a frame are two writers to one socket, and a
    // `Mutex<OwnedWriteHalf>` is what makes that safe rather than a shared
    // borrow the borrow checker cannot see across the two tasks.
    let sink = Arc::new(tokio::sync::Mutex::new(write_half));
    let writer_sink = Arc::clone(&sink);
    let writer_budget = Arc::clone(&budget);
    let writer_playing = Arc::clone(&playing);
    let writer = tokio::spawn(async move {
        let mut sequence: u16 = 0;
        loop {
            if !writer_playing.load(Ordering::Relaxed) {
                tokio::task::yield_now().await;
                continue;
            }
            let sent = writer_budget.load(Ordering::Relaxed);
            if sent >= ACCESS_UNITS {
                // A clean close: the driver reads a real end of stream, which is
                // what ends its loop with `StreamEnd::Closed`.
                let mut sink = writer_sink.lock().await;
                let _ = sink.shutdown().await;
                return;
            }
            writer_budget.store(sent + 1, Ordering::Relaxed);
            let payload = access_unit_payload(sent);
            let packet = rtp_packet(sequence, sent * TICKS_PER_UNIT, &payload, true);
            sequence = sequence.wrapping_add(1);
            let mut sink = writer_sink.lock().await;
            if sink
                .write_all(&interleaved(MEDIA_CHANNEL, &packet))
                .await
                .is_err()
            {
                return;
            }
            let _ = sink.flush().await;
            drop(sink);
            tokio::time::sleep(PACE).await;
        }
    });

    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let mut result = Ok(());
    loop {
        let n = match read_half.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(error) => {
                result = Err(error);
                break;
            }
        };
        buf.extend_from_slice(&chunk[..n]);

        // The driver's streaming loop reads whatever arrived, so the request
        // must be drained one message at a time rather than assumed whole.
        while let Ok((message, used)) = cctv_rtsp::message::Message::parse(&buf) {
            buf.drain(..used);
            let cctv_rtsp::message::Message::Request(request) = message else {
                continue;
            };
            let recorded = Recorded {
                method: request.method.as_str().to_string(),
                uri: request.uri.clone(),
                body: request.body.clone(),
                headers: request
                    .headers
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect(),
            };
            if let Ok(mut log) = shared.requests.lock() {
                log.push(recorded.clone());
            }

            let cseq = request.headers.cseq().unwrap_or(0);
            // The Digest gate applies to every method, as a real camera's does:
            // the first request of the session goes out unauthenticated and is
            // challenged, and the retried one carries the answer.
            let response = if !digest || authorized(&recorded, &recorded.method) {
                respond(&recorded, cseq, &playing, &budget)
            } else {
                response(
                    401,
                    "Unauthorized",
                    cseq,
                    &format!("WWW-Authenticate: {}\r\n", challenge_header()),
                    &[],
                )
            };
            let mut sink = sink.lock().await;
            if sink.write_all(&response).await.is_err() {
                break;
            }
            let _ = sink.flush().await;
        }
    }

    // Read RTSP requests. The media frames are interleaved on this same
    // connection and are consumed by the driver's read loop; once PLAY has been
    // answered the frame stream is not our business here, so we deliberately
    // stop draining this socket (which would eat the frames) and we keep the
    // writer alive: it is still streaming into the wire and the driver needs
    // the frames intact.
    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let mut handled_play = false;
    loop {
        let n = match read_half.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(error) => {
                return Err(error);
            }
        };
        buf.extend_from_slice(&chunk[..n]);

        // Drain complete RTSP messages only; interleaved media frames are
        // `$`-prefixed and are never valid methods, so feeding them to the
        // codec here would swallow the frames.  They travel straight to the
        // driver's read loop.
        let mut i = 0;
        while i < buf.len() {
            match cctv_rtsp::message::Message::parse(&buf[i..]) {
                Ok((message, used)) => {
                    i += used;
                    if let cctv_rtsp::message::Message::Request(request) = message {
                        let recorded = Recorded {
                            method: request.method.as_str().to_string(),
                            uri: request.uri.clone(),
                            body: request.body.clone(),
                            headers: request
                                .headers
                                .iter()
                                .map(|(key, value)| (key.to_string(), value.to_string()))
                                .collect(),
                        };
                        if let Ok(mut log) = shared.requests.lock() {
                            log.push(recorded.clone());
                        }
                        let cseq = request.headers.cseq().unwrap_or(0);
                        // The Digest gate applies to every method, as a real
                        // camera's does: the first request of the session goes
                        // out unauthenticated and is challenged, and the retried
                        // one carries the answer.
                        let response = if !digest || authorized(&recorded, &recorded.method) {
                            respond(&recorded, cseq, &playing, &budget)
                        } else {
                            response(
                                401,
                                "Unauthorized",
                                cseq,
                                &format!("WWW-Authenticate: {}
\n", challenge_header()),
                                &[],
                            )
                        };
                        let mut sink = sink.lock().await;
                        if sink.write_all(&response).await.is_err() {
                            return Err(std::io::Error::other("server write failed"));
                        }
                        let _ = sink.flush().await;

                        // PLAY starts the media; the driver (run_once) now owns
                        // the socket's read side and will read the frames.
                        // Stop reading requests and keep the writer alive.
                        if request.method == cctv_rtsp::message::Method::Play {
                            playing.store(true, Ordering::Relaxed);
                            handled_play = true;
                            break;
                        }
                    }
                }
                Err(cctv_rtsp::message::MessageParseError::Incomplete) => break,
                Err(cctv_rtsp::message::MessageParseError::Malformed(_)) => {
                    i += 1;
                }
            }
        }
        "SETUP" => {
            let transport = request
                .header("Transport")
                .unwrap_or("RTP/AVP/TCP;unicast;interleaved=0-1")
                .to_string();
            response(
                200,
                "OK",
                cseq,
                &format!("Transport: {transport}\r\nSession: {SESSION_ID}\r\n"),
                &[],
            )
        }
        "PLAY" => {
            playing.store(true, Ordering::Relaxed);
            budget.store(ACCESS_UNITS, Ordering::Relaxed);
            response(
                200,
                "OK",
                cseq,
                &format!("Session: {SESSION_ID}\r\nRTP-Info: url=trackID=1;seq=0;rtptime=0\r\n"),
                &[],
            )
        }
        "TEARDOWN" => {
            playing.store(false, Ordering::Relaxed);
            response(200, "OK", cseq, &format!("Session: {SESSION_ID}\r\n"), &[])
        }
        // A keepalive the driver may send on the media connection.  Answered, and
        // never counted as media: the demux consumes it rather than feeding it to
        // the codec.
        "GET_PARAMETER" => response(200, "OK", cseq, "", &[]),
        _ => response(405, "Method Not Allowed", cseq, "", &[]),
    }
}

/// One RTSP response, headers and body, `Content-Length` included.
fn response(status: u16, reason: &str, cseq: u32, extra: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!("RTSP/1.0 {status} {reason}\r\nCSeq: {cseq}\r\n{extra}");
    if !body.is_empty() {
        out.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    out.push_str("\r\n");
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

/// Drive one full session against a fixture: the handshake on one connection,
/// the media on the connection the streaming loop opens, then read.
///
/// This is the seam the milestone test uses.  `now_ms` is injected so the
/// measurement is exact; the returned reader is the one the frames were pushed
/// into, so a caller can assert on what came off the wire.
///
/// **Why `run_once` is used directly.** [`streaming::run`] re-runs
/// `DESCRIBE`/`SETUP`/`PLAY` on the *media* connection, which is what a live
/// camera needs and what would make the fixture send two streams.  The fixture
/// has already answered the handshake on its own connection, so the test starts
/// from the resulting offer and lets the driver's read loop take over.
///
/// **Why it is `async` and not a spawned task.** The fixture closes the
/// connection after a bounded number of units, so the loop ends on an ordinary
/// end of stream in about a second.  Awaiting it keeps the test's assertion on
/// the same clock that stamped the frames, which is what makes the measured rate
/// exact.
pub async fn run_session(
    url: RtspUrl,
    reader: &mut FrameReader,
    now_ms: impl Fn() -> u64,
) -> StreamOutcome {
    let mut session = RtspSession::new(url);
    let described = session.describe().await.expect("DESCRIBE succeeds");
    let offer =
        SessionOffer::pick(described, session.url(), StreamQuality::Main).expect("a video track");
    session
        .setup_and_play(&offer.control_url)
        .await
        .expect("SETUP and PLAY succeed");

    let mut frames = 0u64;
    let stop = stop_channel();
    let reason = match streaming::run_once(
        &mut session,
        &offer,
        reader,
        &stop,
        &now_ms,
        &mut frames,
    )
    .await
    {
        Ok(()) => StreamEnd::Closed,
        // An ordinary end of stream is what the fixture produces: the camera
        // closed the connection, and the loop has nothing left to read.
        Err(CameraError::Offline) => StreamEnd::Closed,
        Err(CameraError::StreamFailed) => StreamEnd::Stalled,
        Err(_) => StreamEnd::Failed,
    };
    StreamOutcome {
        frames,
        dropped: 0,
        reason,
    }
}

/// A `watch` receiver that is never told to stop.
///
/// The fixture closes the connection, so the loop ends on its own; a stop signal
/// that never fires keeps the test honest about *which* of the two ended it.
fn stop_channel() -> streaming::Stop {
    let (_tx, rx) = tokio::sync::watch::channel(false);
    rx
}

#[tokio::test]
async fn the_fixture_answers_the_handshake_and_streams_a_measurable_number_of_frames() {
    let server = FixtureServer::start(false)
        .await
        .expect("the fixture binds");
    let mut reader = FrameReader::new();
    let outcome = run_session(server.url(), &mut reader, clock_ms).await;

    assert_eq!(
        outcome.reason,
        StreamEnd::Closed,
        "the fixture closes the connection, so the loop ends on an ordinary EOF"
    );
    assert_eq!(
        outcome.frames, ACCESS_UNITS as u64,
        "every access unit reached the reader, got {}",
        outcome.frames
    );
    // The frames are measured, not assumed: the reader counted them.
    assert_eq!(reader.received(), ACCESS_UNITS as u64);
    let fps = reader.fps(clock_ms());
    assert!(fps > 0.0, "the reader reports a measured rate, got {fps}");
    assert!(reader.bytes_per_sec(clock_ms()) > 0);

    // The handshake the driver performed, as the camera saw it.
    let requests = server.requests();
    let methods: Vec<&str> = requests.iter().map(|r| r.method.as_str()).collect();
    assert_eq!(
        methods.first().copied(),
        Some("DESCRIBE"),
        "the session asks for the description first"
    );
    assert!(methods.contains(&"SETUP"), "the track is bound");
    assert!(methods.contains(&"PLAY"), "delivery is started");
}

#[tokio::test]
async fn a_digest_challenge_is_answered_and_the_stream_is_served() {
    let server = FixtureServer::start(true).await.expect("the fixture binds");
    assert!(server.digest());

    let url = server.url_with_credentials();
    let mut session = RtspSession::new(url);
    let described = session
        .describe()
        .await
        .expect("DESCRIBE succeeds after a retry");

    // The retry is the proof: the first DESCRIBE was challenged, the second
    // carried the answer, and the description came back.
    let requests = server.requests();
    let describes: Vec<&Recorded> = requests.iter().filter(|r| r.method == "DESCRIBE").collect();
    assert_eq!(
        describes.len(),
        2,
        "the driver sent DESCRIBE twice: once unauthenticated, once with the answer"
    );
    let first = describes[0].header("Authorization");
    assert!(first.is_none(), "the first request carries no answer");
    let second = describes[1]
        .header("Authorization")
        .expect("the retry carries an Authorization header");
    assert!(second.starts_with("Digest "), "{second}");

    // And the session that was challenged streams.
    let offer = SessionOffer::pick(described, session.url(), StreamQuality::Main).expect("a track");
    session
        .setup_and_play(&offer.control_url)
        .await
        .expect("SETUP and PLAY succeed after the retry");

    let mut reader = FrameReader::new();
    let outcome =
        streaming::run(&mut session, &offer, &mut reader, &stop_channel(), clock_ms).await;
    assert!(
        outcome.frames > 0,
        "media flowed through the authenticated session"
    );
}

#[test]
fn a_wrong_digest_answer_is_not_accepted() {
    // The fixture's own gate, exercised directly: a response that does not
    // match the challenge is refused, so the retry the driver performs is a
    // real retry rather than a rubber stamp.
    let wrong = Recorded {
        method: "DESCRIBE".to_string(),
        uri: "rtsp://127.0.0.1/Streaming/Channels/101".to_string(),
        body: Vec::new(),
        headers: vec![(
            "Authorization".to_string(),
            format!(
                "Digest username=\"{USERNAME}\", realm=\"{REALM}\", nonce=\"{NONCE}\", \
                 uri=\"/Streaming/Channels/101\", algorithm=MD5, \
                 response=\"00000000000000000000000000000000\", qop=auth, nc=00000001, \
                 cnonce=\"0a4f113b\""
            ),
        )],
    };
    assert!(
        !authorized(&wrong, "DESCRIBE"),
        "a response that does not match is refused"
    );

    // The same request with the correct answer is accepted.
    let cnonce = "0a4f113b";
    let correct = expected_response("DESCRIBE", "/Streaming/Channels/101", 1, cnonce);
    let right = Recorded {
        headers: vec![(
            "Authorization".to_string(),
            format!(
                "Digest username=\"{USERNAME}\", realm=\"{REALM}\", nonce=\"{NONCE}\", \
                 uri=\"/Streaming/Channels/101\", algorithm=MD5, response=\"{correct}\", \
                 qop=auth, nc=00000001, cnonce=\"{cnonce}\""
            ),
        )],
        ..wrong.clone()
    };
    assert!(
        authorized(&right, "DESCRIBE"),
        "the correct answer is accepted"
    );
}

#[test]
fn the_fixture_sdp_is_parsed_the_way_the_driver_expects() {
    let session = sdp::parse_sdp(&sdp_body());
    let track = session.video().expect("a video track");
    assert_eq!(track.payload_type, PAYLOAD_TYPE);
    assert_eq!(track.encoding, "H264");
    assert_eq!(track.clock_rate, CLOCK_RATE);
    assert_eq!(track.control, "trackID=1");
    assert_eq!(track.packetization_mode, 1);
    assert_eq!(track.parameter_sets.len(), 2, "SPS and PPS");
    assert_eq!(track.parameter_sets[0][0] & 0x1F, 7, "SPS");
    assert_eq!(track.parameter_sets[1][0] & 0x1F, 8, "PPS");
    assert_eq!(session.name, "Fixture Camera");
}

#[test]
fn a_built_rtp_packet_parses_back_into_the_access_unit_it_carries() {
    let payload = access_unit_payload(1);
    let packet = rtp_packet(7, TICKS_PER_UNIT, &payload, true);
    let parsed = cctv_rtp::parse_rtp(&packet).expect("a valid RTP packet");
    assert!(parsed.marker);
    assert_eq!(parsed.payload_type, PAYLOAD_TYPE);
    assert_eq!(parsed.timestamp, TICKS_PER_UNIT);
    assert_eq!(parsed.sequence, 7);
    assert_eq!(parsed.payload, payload.as_slice());

    let mut depacketizer = cctv_rtp::H264Depacketizer::new();
    let unit = depacketizer
        .push(parsed.payload, parsed.marker, parsed.timestamp)
        .expect("the marker ends the access unit");
    assert!(
        unit.nal_units
            .iter()
            .any(|nal| nal.first() == Some(&NAL_HEADER)),
        "the IDR slice is in the emitted unit"
    );
}

#[test]
fn an_interleaved_frame_round_trips_through_the_demux() {
    let packet = rtp_packet(3, 0, &access_unit_payload(0), true);
    let frame = interleaved(MEDIA_CHANNEL, &packet);
    let (piece, used) = streaming::demux::split(&frame)
        .expect("a piece")
        .expect("a whole frame");
    let streaming::Piece::Media { channel, packet } = piece else {
        panic!("not media");
    };
    assert_eq!(channel, MEDIA_CHANNEL);
    assert_eq!(packet, packet);
    assert_eq!(used, frame.len());
}

/// A camera that answers nothing is a typed error, not a panic.
#[tokio::test]
async fn a_refused_connection_is_an_offline_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("binds");
    let addr = listener.local_addr().expect("an address");
    drop(listener);
    let url = RtspUrl::parse(&format!("rtsp://{addr}/stream")).expect("a url");
    let mut session = RtspSession::new(url);
    assert!(matches!(
        session.describe().await,
        Err(CameraError::Offline)
    ));
}

/// The fixture's media is bounded, so a session cannot run forever.
#[tokio::test]
async fn the_media_budget_is_finite_and_the_reader_counts_every_unit() {
    let server = FixtureServer::start(false)
        .await
        .expect("the fixture binds");
    let mut reader = FrameReader::new();
    let outcome = run_session(server.url(), &mut reader, clock_ms).await;
    assert!(
        outcome.frames <= ACCESS_UNITS as u64,
        "the fixture sent a bounded number of units, got {}",
        outcome.frames
    );
    assert_eq!(outcome.frames, reader.received());

}
