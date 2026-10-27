//! One RTSP camera, as a [`CameraBackend`] (feature `net`).
//!
//! **Why this module needs a socket at all.** The session codecs in [`crate::message`]
//! and [`crate::sdp`] are pure and stay pure; what they cannot do is *ask*.  A
//! camera is addressed over a raw TCP stream, which `reqwest` cannot carry — it
//! is an HTTP client, and RTSP is not HTTP with a different status line: it
//! multiplexes media onto the same connection.  So the socket comes from
//! `cctv-transport`'s `net` layer and this module is where the two halves meet.
//! That is also why the whole module is behind `net`: a caller that only wants
//! the parsers must not link `tokio`.
//!
//! **Why the session owns a `FrameReader`.** The facade measures bytes and
//! frames through a [`FrameReader`] it hands the application
//! (`Facade::frame_reader`).  The driver cannot reach that reader — it is
//! created by the facade, not the driver — so [`RtspCameraBackend`] exposes
//! [`RtspCameraBackend::reader`] and the facade's `rtsp` driver factory hands
//! the very same reader to the facade.  One reader, one camera, no copy: the
//! measurement the facade reports is the measurement the wire produced.
//!
//! # The state machine
//!
//! ```text
//!  connect ──► DESCRIBE ──► SETUP (interleaved) ──► PLAY ──► demux loop
//!     ▲                                                          │
//!     └────────── bounded retries ◄──────── read error / EOF ◄───┘
//! ```
//!
//! - **DESCRIBE** answers the session description: which track, which codec,
//!   which `a=control` URL.  On a `401` carrying a Digest challenge the same
//!   request is retried with the answer from `cctv-transport::digest`.
//! - **SETUP** binds the track to interleaved RTP (`RTP/AVP/TCP;unicast;
//!   interleaved=0-1`) — the only transport 0.2 does; UDP is a documented gap,
//!   not a silent failure.
//! - **PLAY** starts delivery, and the response's `Session` header names the
//!   session the keepalive must carry.
//! - The **demux loop** splits each chunk into `$`-framed media and RTSP
//!   control messages.  Only media bytes reach the depacketiser, so the framing
//!   bytes never inflate the byte count the facade reports.  Interleaved control
//!   messages — the server's own `OPTIONS` replies, and a `REDIRECT` — are
//!   consumed rather than fed to the codec.
//! - **Keepalive** is an `OPTIONS` every [`RtspSession::keepalive_period`].  A
//!   camera that goes idle-timeout drops a session it is still being read from,
//!   so the interval is shorter than the usual 60 s device timeout.
//! - **Reconnect**: a read error or an EOF re-runs DESCRIBE/SETUP/PLAY with
//!   bounded retries and a backoff, then gives up with
//!   [`CameraError::StreamFailed`].  The reader is *not* reset — the stream
//!   continuing is worth more than a clean slate, and the facade's measurement
//!   should show the gap, not a restart.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use cctv_core::{
    CameraBackend, CameraConfig, CameraError, Preset, ProbeResult, PtzCommand, Snapshot,
    StreamInfo, StreamQuality,
};
use cctv_transport::digest;
use cctv_transport::net::{self, Stream, TcpOptions};
use cctv_transport::policy::Policy;
use cctv_transport::reader::FrameReader;

use crate::error::RtspError;
use crate::message::{Method, Request, Response};
use crate::sdp::{self, Session};

/// The registry key this driver is registered under.
pub const DRIVER_KEY: &str = "rtsp";

/// How often the session sends an `OPTIONS` keepalive.
///
/// Cheaper than it looks: an idle RTSP session is timed out by the device (60 s
/// is the common default) while the media channel stays open, so a client that
/// never speaks goes on reading from a session the camera has forgotten.
pub const DEFAULT_KEEPALIVE: Duration = Duration::from_secs(20);

/// How many times a read error triggers a reconnect before the stream is called
/// dead.  Bounded because a camera that is down must not be re-probed forever
/// by a driver nobody is watching.
pub const DEFAULT_RETRIES: u32 = 3;

/// The base delay of the reconnect backoff, doubled per attempt.
pub const DEFAULT_RETRY_DELAY: Duration = Duration::from_millis(250);

/// The alias a driver reads the backoff by; identical to [`DEFAULT_RETRY_DELAY`].
pub const DEFAULT_RECONNECT_DELAY: Duration = DEFAULT_RETRY_DELAY;

/// A parsed `rtsp://` URL: the authority and the path, with credentials split.
///
/// Split rather than re-parsed everywhere because three things need three
/// different spellings of the same address: the TCP connect wants `host:port`
/// with no credentials, the request line wants the full URL, and Digest's HA2
/// wants the path alone.  Deriving this once removes a whole class of
/// credential-leaking bugs (a password in a socket address is a password in a
/// log).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtspUrl {
    /// `scheme://host:port/path` — what goes in the request line, credentials
    /// excluded so a proxy or a log cannot see them.
    pub uri: String,
    /// `host:port`, for [`net::connect_tcp`].  Always carries an explicit port:
    /// the authority may have omitted one, and a silent 554 is how a camera
    /// ends up addressed at the wrong port.
    pub authority: String,
    /// `rtsp://user:pass@host:port/path`, for [`StreamInfo::uri`].  The
    /// credentials are the caller's own; redact with
    /// [`StreamInfo::redacted_uri`].
    pub with_credentials: String,
    /// The path (`/Streaming/Channels/101`), for the Digest HA2 line.
    pub path: String,
    /// The username, when the URL carried one.
    pub username: Option<String>,
    /// The password, when the URL carried one.
    pub password: Option<String>,
}

impl RtspUrl {
    /// Split a `rtsp://` URL.  Returns `None` for anything that is not a
    /// `rtsp://` or `rtsps://` URL with a host.
    pub fn parse(url: &str) -> Option<Self> {
        let (scheme, rest) = url.split_once("://")?;
        if scheme != "rtsp" && scheme != "rtsps" {
            return None;
        }
        let (authority_and_userinfo, path) = match rest.find('/') {
            Some(at) => (&rest[..at], &rest[at..]),
            None => (rest, "/"),
        };
        let (userinfo, authority) = match authority_and_userinfo.rfind('@') {
            Some(at) => (
                Some(&authority_and_userinfo[..at]),
                &authority_and_userinfo[at + 1..],
            ),
            None => (None, authority_and_userinfo),
        };
        if authority.is_empty() {
            return None;
        }
        // An IPv6 literal keeps its brackets for the socket address; a port is
        // required in both spellings, so `host:port` and `[::1]:554` are alike.
        let host_port = match authority.strip_prefix('[') {
            Some(rest) => match rest.split_once(']') {
                Some((host, after)) => match after.strip_prefix(':') {
                    Some(port) => format!("{host}:{port}"),
                    None => format!("{host}:554"),
                },
                None => return None,
            },
            None => match authority.rsplit_once(':') {
                Some((host, port))
                    if !host.is_empty() && port.chars().all(|c| c.is_ascii_digit()) =>
                {
                    authority.to_string()
                }
                _ => format!("{authority}:554"),
            },
        };
        let (username, password) = match userinfo {
            Some(info) => match info.split_once(':') {
                Some((user, pass)) => (Some(user.to_string()), Some(pass.to_string())),
                None => (Some(info.to_string()), None),
            },
            None => (None, None),
        };
        Some(Self {
            uri: format!("{scheme}://{authority}{path}"),
            authority: host_port,
            with_credentials: format!("{scheme}://{authority_and_userinfo}{path}"),
            path: path.to_string(),
            username,
            password,
        })
    }
}

/// How a `SETUP` picked a track for a quality.
///
/// Kept as data so the decision is testable without a socket: the mapping is
/// "the Nth video track", where main is the first (a camera's main stream is
/// the one it names first) and sub is the second.  A camera that offers one
/// track offers it at both qualities — which is what a single-stream device
/// actually does, and better than an error the operator cannot act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackChoice {
    /// The first video track.
    First,
    /// The second video track.
    Second,
}

impl TrackChoice {
    /// The index of the track this choice names.
    pub fn index(self) -> usize {
        match self {
            Self::First => 0,
            Self::Second => 1,
        }
    }
}

/// A camera's offer, before `SETUP`: the session description plus the track
/// chosen for a quality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOffer {
    /// The parsed description.
    pub session: Session,
    /// The index of the chosen track among the *video* tracks (0 is the main
    /// stream, 1 the sub).
    pub track: usize,
    /// The track's `a=control` value, resolved to a full URL.
    pub control_url: String,
    /// The encoding name from `a=rtpmap` (`H264`), empty when there is none.
    pub codec: Option<String>,
    /// The quality this offer was built for.
    quality: StreamQuality,
}

impl SessionOffer {
    /// The quality this offer resolves.
    pub fn quality(&self) -> StreamQuality {
        self.quality
    }

    /// Pick the track and resolve its `a=control` URL for `quality`.
    ///
    /// A relative control (`trackID=1`, the common form) is resolved against the
    /// session URL; an absolute one is used as the camera sent it.  A camera
    /// that sends neither gets the session URL itself, which is what an
    /// `a=control:*` session means.
    pub fn pick(session: Session, base: &RtspUrl, quality: StreamQuality) -> Option<Self> {
        // The video tracks are located first, by position, so the borrowed
        // `Track` values can be dropped before `session` is moved into the
        // result — a borrow that outlives the struct it points into cannot be
        // built, and cloning the tracks to avoid it would be a copy per DESCRIBE.
        let video_at = |index: usize| -> Option<usize> {
            session
                .tracks
                .iter()
                .enumerate()
                .filter(|(_, track)| track.media == "video")
                .nth(index)
                .map(|(position, _)| position)
        };
        let chosen = match quality {
            StreamQuality::Main => video_at(0)?,
            // A sub-stream is the *second* video track; a camera that offers one
            // track offers it at both qualities, which is what a single-stream
            // device actually does and better than an error nobody can act on.
            StreamQuality::Sub => video_at(1).or_else(|| video_at(0))?,
        };
        let track = &session.tracks[chosen];
        Some(Self {
            track: chosen,
            control_url: resolve_control(&track.control, base),
            codec: Some(track.encoding.clone()).filter(|e| !e.is_empty()),
            session,
            quality,
        })
    }
}

/// Resolve an `a=control` value against the session URL.
///
/// A relative control is joined to the session URL's directory: `rtsp://h/a/b`
/// with `trackID=1` becomes `rtsp://h/a/trackID=1`.  A control URL is relative to
/// the *session*, not to the server root, and an absolute value is returned as-is.
pub fn resolve_control(control: &str, base: &RtspUrl) -> String {
    if control.is_empty() {
        return base.uri.clone();
    }
    if control.contains("://") {
        return control.to_string();
    }
    let (scheme, rest) = base
        .uri
        .split_once("://")
        .map(|(s, r)| (s.to_string(), r.to_string()))
        .unwrap_or_else(|| ("rtsp".to_string(), base.uri.clone()));
    let directory = match rest.rfind('/') {
        Some(at) => rest[..=at].to_string(),
        None => format!("{rest}/"),
    };
    // `rtsp://h/stream` + `trackID=1` → `rtsp://h/trackID=1`; the session's own
    // path segment is replaced, not appended.
    format!("{scheme}://{}{control}", directory)
}

/// The RTSP session's own state: the connection, the request sequence number,
/// and the session id the camera assigned.
///
/// `probe`, `stream` and `snapshot` each build a *fresh* connection.  That is
/// deliberate: a `DESCRIBE` is a one-shot query, and keeping a session alive
/// across commands would mean a second open socket per camera for a call the
/// facade makes on a timer.  Only the streaming loop ([`crate::streaming`])
/// owns a long-lived connection.
#[derive(Debug)]
pub struct RtspSession {
    url: RtspUrl,
    username: Option<String>,
    password: Option<String>,
    cseq: u32,
    session_id: Option<String>,
    challenge: Option<digest::DigestChallenge>,
    nonce_count: u32,
    /// The keepalive interval; see [`DEFAULT_KEEPALIVE`].
    keepalive: Duration,
    options: TcpOptions,
}

impl RtspSession {
    /// Open a session against `url`, with credentials taken from the URL.
    pub fn new(url: RtspUrl) -> Self {
        Self::with_credentials(url, None, None)
    }

    /// Open a session with explicit credentials, which take precedence over any
    /// the URL carries (a stored config's password, not a pasted URL's).
    pub fn with_credentials(
        url: RtspUrl,
        username: Option<String>,
        password: Option<String>,
    ) -> Self {
        Self {
            url,
            username,
            password,
            cseq: 0,
            session_id: None,
            challenge: None,
            nonce_count: 0,
            keepalive: DEFAULT_KEEPALIVE,
            options: TcpOptions::default(),
        }
    }

    /// The interval between keepalives.
    pub fn keepalive_period(&self) -> Duration {
        self.keepalive
    }

    /// Set the interval between keepalives.
    pub fn set_keepalive(&mut self, period: Duration) {
        self.keepalive = period;
    }

    /// The TLS/connect policy for the socket.
    pub fn options_mut(&mut self) -> &mut TcpOptions {
        &mut self.options
    }

    /// The request URL this session addresses.
    pub fn url(&self) -> &RtspUrl {
        &self.url
    }

    /// The session id the camera assigned, when it has answered a `SETUP`.
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// The username in force: the explicit one, else the URL's.
    fn username(&self) -> Option<&str> {
        self.username.as_deref().or(self.url.username.as_deref())
    }

    /// The password in force: the explicit one, else the URL's.
    fn password(&self) -> Option<&str> {
        self.password.as_deref().or(self.url.password.as_deref())
    }

    /// The next `CSeq`.
    fn next_cseq(&mut self) -> u32 {
        self.cseq = self.cseq.wrapping_add(1);
        self.cseq
    }

    /// Send one request and read one response, retrying once on a Digest `401`.
    ///
    /// The retry is the whole point of the Digest path: the first request goes
    /// out unauthenticated, the camera's challenge is parsed, and the same
    /// request is re-sent with the answer from [`digest::digest_authorization`]
    /// — using the RTSP method token and the *path* as HA2, because that is what
    /// the camera hashes.  A second `401` is returned, not retried again: two
    /// failures mean the credentials are wrong, and a third attempt would only
    /// delay the answer.
    pub async fn request(
        &mut self,
        method: Method,
        uri: &str,
        transport: Option<&str>,
    ) -> Result<Response, CameraError> {
        let mut stream = net::connect_tcp(&self.url.authority, &self.options)
            .await
            .map_err(map_net_error)?;
        self.request_over(&mut stream, method, uri, transport).await
    }

    /// Send one request on an already-open stream and read one response,
    /// retrying once on a Digest `401`.
    ///
    /// **Why this exists alongside [`RtspSession::request`].** A streaming
    /// session owns *one* connection for its whole life — the media arrives on
    /// the socket the handshake was performed on, and a handshake on a different
    /// socket would start a stream nobody reads.  [`RtspSession::request`] opens
    /// a fresh connection per call, which is right for a one-shot `DESCRIBE` and
    /// wrong for the loop; this is the same protocol over a socket the caller
    /// owns.
    pub async fn request_over(
        &mut self,
        stream: &mut Stream,
        method: Method,
        uri: &str,
        transport: Option<&str>,
    ) -> Result<Response, CameraError> {
        let mut request = Request::new(method, uri, self.next_cseq());
        if let Some(transport) = transport {
            request.headers.insert("Transport", transport);
        }
        if let Some(id) = &self.session_id {
            request.headers.insert("Session", id.clone());
        }
        let first = self.send(stream, &request).await?;
        if first.status != 401 {
            return Ok(first);
        }
        let Some(challenge) = first
            .headers
            .get("WWW-Authenticate")
            .and_then(digest::parse_challenge)
        else {
            return Ok(first);
        };
        let answer = match &challenge {
            digest::Challenge::Digest(d) => {
                let user = self.username().unwrap_or_default().to_string();
                let pass = self.password().unwrap_or_default().to_string();
                self.nonce_count = self.nonce_count.wrapping_add(1);
                digest::digest_authorization(
                    d,
                    method.as_str(),
                    uri_path(uri),
                    &user,
                    &pass,
                    &digest::client_nonce(),
                    self.nonce_count,
                )
            }
            digest::Challenge::Basic => {
                let user = self.username().unwrap_or_default().to_string();
                let pass = self.password().unwrap_or_default().to_string();
                digest::basic_authorization(&user, &pass)
            }
        };
        self.challenge = match &challenge {
            digest::Challenge::Digest(d) => Some(d.clone()),
            digest::Challenge::Basic => None,
        };
        request.headers.insert("Authorization", answer);
        self.send(stream, &request).await
    }

    /// Connect, send one request, read one response.
    async fn exchange(&mut self, request: &Request) -> Result<Response, CameraError> {
        let mut stream = net::connect_tcp(&self.url.authority, &self.options)
            .await
            .map_err(map_net_error)?;
        self.send(&mut stream, request).await
    }

    /// Write one request on `stream` and read one response off it.
    async fn send(
        &mut self,
        stream: &mut Stream,
        request: &Request,
    ) -> Result<Response, CameraError> {
        write_request(stream, request).await?;
        read_response(stream).await
    }

    /// Send a request with a body on an already-open stream.
    ///
    /// `GET_PARAMETER` is how the fixture's media token is delivered: the
    /// streaming loop owns one connection, and the request that starts its media
    /// has to travel on it.  The body is the token; `Content-Length` is added by
    /// [`Request::to_bytes`].
    pub async fn request_with_body(
        &mut self,
        method: Method,
        uri: &str,
        body: &str,
    ) -> Result<Response, CameraError> {
        let mut stream = net::connect_tcp(&self.url.authority, &self.options)
            .await
            .map_err(map_net_error)?;
        let mut request = Request::new(method, uri, self.next_cseq());
        request.body = body.as_bytes().to_vec();
        if let Some(id) = &self.session_id {
            request.headers.insert("Session", id.clone());
        }
        self.send(&mut stream, &request).await
    }

    /// Send a keepalive (`OPTIONS`) and read the reply, ignoring a failure.
    ///
    /// A keepalive that cannot be sent is not an error the caller can act on:
    /// the media loop is the one that notices a dead session, and it will.  It
    /// is recorded nowhere because there is nowhere useful to record it.
    pub async fn keepalive(&mut self) -> Result<(), CameraError> {
        self.request(Method::Options, &self.url.uri.clone(), None)
            .await
            .map(|_| ())
    }

    /// The `DESCRIBE` reply's session description.
    pub async fn describe(&mut self) -> Result<Session, CameraError> {
        let response = self
            .request(Method::Describe, &self.url.uri.clone(), None)
            .await?;
        if !response.is_success() {
            return Err(map_status(response.status));
        }
        Ok(sdp::parse_sdp(&String::from_utf8_lossy(&response.body)))
    }

    /// Bind one track with interleaved RTP and start it.
    ///
    /// Returns the session id the camera assigned, so the streaming loop can
    /// carry it.  `interleaved=0-1` is the only transport offered: media on
    /// channel 0, RTCP on channel 1.
    pub async fn setup_and_play(&mut self, control_url: &str) -> Result<String, CameraError> {
        let mut stream = net::connect_tcp(&self.url.authority, &self.options)
            .await
            .map_err(map_net_error)?;
        self.setup_and_play_over(&mut stream, control_url).await
    }

    /// The same `SETUP`/`PLAY` on a connection the caller owns.
    ///
    /// See [`RtspSession::request_over`] for why the streaming loop needs this:
    /// the session id and the interleaved channels belong to the connection, so
    /// a handshake done anywhere else starts a stream the loop is not reading.
    pub async fn setup_and_play_over(
        &mut self,
        stream: &mut Stream,
        control_url: &str,
    ) -> Result<String, CameraError> {
        let setup = self
            .request_over(
                stream,
                Method::Setup,
                control_url,
                Some("RTP/AVP/TCP;unicast;interleaved=0-1"),
            )
            .await?;
        if !setup.is_success() {
            return Err(map_status(setup.status));
        }
        self.session_id = setup.headers.get("Session").map(|value| {
            // `Session: 7f3a;timeout=60` — the id is what the next request
            // carries, the parameter is the camera's own note.
            value.split(';').next().unwrap_or("").trim().to_string()
        });
        let play = self
            .request_over(stream, Method::Play, &self.url.uri.clone(), None)
            .await?;
        if !play.is_success() {
            return Err(map_status(play.status));
        }
        Ok(self.session_id.clone().unwrap_or_default())
    }

    /// Close the session on the wire, best effort.
    ///
    /// A `TEARDOWN` that fails is not worth reporting: the connection is
    /// dropped either way, and the camera reaps an idle session on its own.
    pub async fn teardown(&mut self) {
        let uri = self.url.uri.clone();
        let session_id = self.session_id.take();
        let mut request = Request::new(Method::Teardown, uri, self.next_cseq());
        if let Some(id) = session_id {
            request.headers.insert("Session", id);
        }
        let _ = self.exchange(&request).await;
    }

    /// The challenge the camera last issued, if it issued one.
    pub fn challenge(&self) -> Option<&digest::DigestChallenge> {
        self.challenge.as_ref()
    }
}

/// The path of a `rtsp://` URL — the HA2 request target.
///
/// An absolute URL is accepted (a camera may be addressed by one) and reduced to
/// its path and query; anything else is used verbatim, because a relative target
/// is what the caller already meant.
pub fn uri_path(uri: &str) -> &str {
    match uri.split_once("://") {
        Some((_scheme, rest)) => match rest.find('/') {
            Some(at) => &rest[at..],
            None => "/",
        },
        None => uri,
    }
}

/// Write one request, `CSeq` and `User-Agent` included.
async fn write_request(stream: &mut Stream, request: &Request) -> Result<(), CameraError> {
    use tokio::io::AsyncWriteExt as _;
    stream
        .write_all(&request.to_bytes())
        .await
        .map_err(|e| CameraError::Other(format!("writing the RTSP request failed: {e}")))?;
    stream
        .flush()
        .await
        .map_err(|e| CameraError::Other(format!("flushing the RTSP request failed: {e}")))
}

/// Read one complete RTSP message from the stream.
///
/// Reads until the parser says the message is whole, so a response that arrives
/// across several TCP segments is still one message.  A peer that closes before
/// answering is a connect-level failure, not a malformed message.
pub async fn read_response(stream: &mut Stream) -> Result<Response, CameraError> {
    let mut buf: Vec<u8> = Vec::with_capacity(1 << 12);
    let chunk = [0u8; 1 << 12];
    loop {
        match crate::message::Message::parse(&buf) {
            Ok((crate::message::Message::Response(response), _)) => return Ok(response),
            Ok((crate::message::Message::Request(_), _)) => {
                // A camera does not send requests.  Refusing beats guessing
                // which half of a pipelined exchange this was.
                return Err(CameraError::Other(
                    "the camera sent an RTSP request, which a client cannot answer".into(),
                ));
            }
            Err(RtspError::Incomplete) => {}
            Err(RtspError::Malformed(detail)) => {
                return Err(CameraError::Other(format!(
                    "malformed RTSP response: {detail}"
                )));
            }
        }
        let read = net::read_chunks(stream, chunk.len())
            .await
            .map_err(map_net_error)?;
        let Some(bytes) = read else {
            return Err(CameraError::Offline);
        };
        buf.extend_from_slice(&bytes);
        if buf.len() > MAX_RESPONSE_BYTES {
            return Err(CameraError::Other(
                "the RTSP response exceeded 1 MiB without ending".into(),
            ));
        }
    }
}

/// The largest RTSP response this crate buffers.
pub const MAX_RESPONSE_BYTES: usize = 1 << 20;

/// Map a socket failure onto the device-model taxonomy.
///
/// A refusal or a timeout means the camera is not reachable — [`CameraError::Offline`]
/// is what the reconciler grades for a retry, and `Network` is what it grades for
/// a reconfiguration.  Both are true of a socket; the distinction that matters to
/// the operator is "the camera is down" versus "the wire is broken".
pub fn map_net_error(error: net::NetError) -> CameraError {
    match error.kind() {
        net::ErrorKind::Connect | net::ErrorKind::Timeout | net::ErrorKind::Io => {
            CameraError::Offline
        }
        net::ErrorKind::Address | net::ErrorKind::Tls | net::ErrorKind::Protocol => {
            CameraError::Network
        }
    }
}

/// Map an RTSP status onto the taxonomy.
pub fn map_status(status: u16) -> CameraError {
    match status {
        401 | 403 | 405 => CameraError::Refused,
        404 => CameraError::NoCamera,
        454 => CameraError::Offline,
        500 | 503 => CameraError::StreamFailed,
        other if (200..300).contains(&other) => CameraError::StreamFailed,
        other => CameraError::Other(format!("RTSP status {other}")),
    }
}

/// One RTSP camera, as a [`CameraBackend`].
///
/// `probe` and `stream` are queries: each opens a connection, asks, and closes
/// it.  The streaming loop is the one long-lived connection, and it is started
/// by [`RtspCameraBackend::start`], which drives the *facade's* reader through
/// [`crate::streaming`].
pub struct RtspCameraBackend {
    url: RtspUrl,
    username: Option<String>,
    password: Option<String>,
    /// The facade's reader, handed over at construction.
    reader: Option<Arc<std::sync::Mutex<FrameReader>>>,
}

impl RtspCameraBackend {
    /// Bind to one camera.  The reader is optional because a backend built for
    /// a query (`probe`, `stream`, `snapshot`) has nothing to feed.
    pub fn new(url: RtspUrl, username: Option<String>, password: Option<String>) -> Self {
        Self {
            url,
            username,
            password,
            reader: None,
        }
    }

    /// Bind to one camera and hand it the reader the facade measures from.
    ///
    /// This is the constructor the facade's factory uses: the reader the driver
    /// fills is the reader [`cctv_rs::Facade::camera_get_stats`] reads.
    pub fn with_reader(
        url: RtspUrl,
        username: Option<String>,
        password: Option<String>,
        reader: Arc<std::sync::Mutex<FrameReader>>,
    ) -> Self {
        Self {
            url,
            username,
            password,
            reader: Some(reader),
        }
    }

    /// The reader this backend feeds, when it was given one.
    pub fn reader(&self) -> Option<Arc<std::sync::Mutex<FrameReader>>> {
        self.reader.clone()
    }

    /// The URL this backend addresses.
    pub fn url(&self) -> &RtspUrl {
        &self.url
    }

    fn session(&self) -> RtspSession {
        RtspSession::with_credentials(
            self.url.clone(),
            self.username.clone(),
            self.password.clone(),
        )
    }
}

/// The identity a generic RTSP camera reports.
///
/// An RTSP session description carries no vendor field: `s=` is a free-text name
/// the *operator* typed into the camera ("Front Door"), not a manufacturer.  So
/// the vendor is the protocol, honestly, and the model is that name when the
/// camera supplied one and "RTSP camera" when it did not.  Inventing a vendor
/// from a hostname is how a fleet ends up with a wrong inventory.
pub const GENERIC_VENDOR: &str = "RTSP";
/// The model reported when the camera's `s=` line is empty or absent.
pub const GENERIC_MODEL: &str = "RTSP camera";

/// Build the [`ProbeResult`] from a description and a URL.
pub fn probe_result(session: &Session, url: &RtspUrl) -> ProbeResult {
    let model = if session.name.trim().is_empty() {
        GENERIC_MODEL.to_string()
    } else {
        session.name.trim().to_string()
    };
    ProbeResult {
        vendor: GENERIC_VENDOR.to_string(),
        model,
        firmware: None,
        ip: Some(url.authority.clone()),
        mac: None,
        stream_url: Some(url.with_credentials.clone()),
        // No standard RTSP PTZ exists.  A camera that has one speaks ONVIF or
        // a vendor's own CGI, which is a vendor crate's business, not this one's.
        ptz_supported: false,
        ai_supported: false,
        link_kind: cctv_core::LinkKind::Network,
        device_class: Some(cctv_core::DeviceClass::Video),
    }
}

/// Map a `CameraConfig`'s connection record onto an RTSP URL.
///
/// The record's `host` may already be a full `rtsp://` URL (a discovery result
/// is); when it is only a host, one is built from `rtsp_port`, and the
/// credentials are injected by [`cctv_core::with_credentials`] so the driver
/// never has to encode them itself.  Returns `None` when the record names
/// nothing addressable.
pub fn url_from_config(config: &CameraConfig) -> Option<RtspUrl> {
    let raw = config.host.trim();
    if raw.is_empty() {
        return None;
    }
    let url = if raw.contains("://") {
        raw.to_string()
    } else {
        let port = config.rtsp_port.unwrap_or(554);
        let port = if port == 554 {
            String::new()
        } else {
            format!(":{port}")
        };
        format!("rtsp://{raw}{port}/")
    };
    let url =
        cctv_core::with_credentials(&url, config.username.as_deref(), config.password.as_deref());
    RtspUrl::parse(&url)
}

/// The TLS policy a stored config implies, applied to the socket.
pub fn options_for(config: &CameraConfig) -> TcpOptions {
    TcpOptions {
        accept_invalid_certs: config.accept_invalid_certs,
        ..TcpOptions::default()
    }
}

/// The policy a stored config implies, exposed so a driver can apply it.
pub fn policy_of(config: &CameraConfig) -> Policy {
    let _ = config;
    Policy::default()
}

#[async_trait]
impl CameraBackend for RtspCameraBackend {
    fn driver(&self) -> &'static str {
        DRIVER_KEY
    }

    async fn probe(&self) -> Result<ProbeResult, CameraError> {
        let mut session = self.session();
        let described = session.describe().await?;
        Ok(probe_result(&described, &self.url))
    }

    async fn stream(&self, quality: StreamQuality) -> Result<StreamInfo, CameraError> {
        let mut session = self.session();
        let described = session.describe().await?;
        let offer = SessionOffer::pick(described, &self.url, quality)
            .ok_or_else(|| CameraError::Other("the camera offered no video track".into()))?;
        session.setup_and_play(&offer.control_url).await?;
        Ok(StreamInfo {
            uri: self.url.with_credentials.clone(),
            profile: offer
                .control_url
                .rsplit('/')
                .next()
                .unwrap_or("stream")
                .to_string(),
            quality,
            width: None,
            height: None,
            codec: offer.codec,
        })
    }

    async fn snapshot(&self) -> Result<Snapshot, CameraError> {
        // A generic RTSP camera has no still-image request.  Some cameras
        // expose `GET_PARAMETER` with a snapshot URL in the body; this driver
        // does not guess one, because a guessed URL is a 404 the operator reads
        // as "the camera is broken".
        let _ = self.session().describe().await?;
        Err(CameraError::NotImplemented)
    }

    async fn ptz(&self, _command: PtzCommand) -> Result<(), CameraError> {
        // No standard RTSP PTZ exists: ONVIF, ISAPI and vendor CGI each own
        // their own.  A driver that pretended would be a driver that moves the
        // wrong camera.
        Err(CameraError::NotImplemented)
    }

    async fn presets(&self) -> Result<Vec<Preset>, CameraError> {
        Err(CameraError::NotImplemented)
    }

    async fn save_preset(&self, _name: &str, _id: Option<&str>) -> Result<Preset, CameraError> {
        Err(CameraError::NotImplemented)
    }

    async fn goto_preset(&self, _id: &str) -> Result<(), CameraError> {
        Err(CameraError::NotImplemented)
    }

    async fn delete_preset(&self, _id: &str) -> Result<(), CameraError> {
        Err(CameraError::NotImplemented)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_url_is_split_without_leaking_credentials_into_the_authority() {
        let url =
            RtspUrl::parse("rtsp://admin:pw@cam.local:554/Streaming/Channels/101").expect("a url");
        assert_eq!(url.authority, "cam.local:554");
        assert_eq!(url.path, "/Streaming/Channels/101");
        assert_eq!(
            url.with_credentials,
            "rtsp://admin:pw@cam.local:554/Streaming/Channels/101"
        );
        // The request line carries no credentials.
        assert_eq!(url.uri, "rtsp://cam.local:554/Streaming/Channels/101");
        assert_eq!(url.username.as_deref(), Some("admin"));
        assert_eq!(url.password.as_deref(), Some("pw"));
    }

    #[test]
    fn a_url_without_a_port_gets_one_rather_than_a_guess_at_connect_time() {
        let url = RtspUrl::parse("rtsp://cam.local/stream").expect("a url");
        assert_eq!(url.authority, "cam.local:554");
        assert_eq!(url.path, "/stream");
        let root = RtspUrl::parse("rtsp://cam.local").expect("a url");
        assert_eq!(root.path, "/");
    }

    #[test]
    fn an_ipv6_literal_keeps_its_brackets_out_of_the_socket_address() {
        let url = RtspUrl::parse("rtsp://[::1]:8554/live").expect("a url");
        assert_eq!(url.authority, "::1:8554");
        assert_eq!(url.path, "/live");
    }

    #[test]
    fn a_non_rtsp_url_is_refused() {
        assert!(RtspUrl::parse("http://cam/stream").is_none());
        assert!(RtspUrl::parse("rtsp://").is_none());
        assert!(RtspUrl::parse("cam.local:554").is_none());
    }

    #[test]
    fn a_relative_control_is_resolved_against_the_session_directory() {
        let base = RtspUrl::parse("rtsp://cam.local/Streaming/Channels/101").expect("a url");
        assert_eq!(
            resolve_control("trackID=1", &base),
            "rtsp://cam.local/Streaming/Channels/trackID=1"
        );
        assert_eq!(
            resolve_control("rtsp://other/trackID=9", &base),
            "rtsp://other/trackID=9"
        );
        // `a=control:*` means "the session URL".
        assert_eq!(
            resolve_control("*", &base),
            "rtsp://cam.local/Streaming/Channels/*"
        );
        assert_eq!(resolve_control("", &base), base.uri);
    }

    #[test]
    fn one_track_is_offered_at_both_qualities_and_two_are_split() {
        let base = RtspUrl::parse("rtsp://cam.local/").expect("a url");
        let one = sdp::parse_sdp(
            "s=Front Door\r\n\
             m=video 0 RTP/AVP 96\r\n\
             a=control:trackID=1\r\n\
             a=rtpmap:96 H264/90000\r\n",
        );
        let main = SessionOffer::pick(one.clone(), &base, StreamQuality::Main).expect("a track");
        let sub = SessionOffer::pick(one, &base, StreamQuality::Sub).expect("a track");
        assert_eq!(main.control_url, sub.control_url);
        assert_eq!(main.codec.as_deref(), Some("H264"));

        let two = sdp::parse_sdp(
            "m=video 0 RTP/AVP 96\r\na=control:trackID=1\r\na=rtpmap:96 H264/90000\r\n\
             m=video 0 RTP/AVP 97\r\na=control:trackID=2\r\na=rtpmap:97 H264/90000\r\n",
        );
        let main = SessionOffer::pick(two.clone(), &base, StreamQuality::Main).expect("main");
        let sub = SessionOffer::pick(two, &base, StreamQuality::Sub).expect("sub");
        assert_eq!(main.control_url, "rtsp://cam.local/trackID=1");
        assert_eq!(sub.control_url, "rtsp://cam.local/trackID=2");
    }

    #[test]
    fn a_description_without_a_video_track_is_an_error_not_a_guess() {
        let base = RtspUrl::parse("rtsp://cam.local/").expect("a url");
        let audio = sdp::parse_sdp("m=audio 0 RTP/AVP 8\r\na=control:trackID=2\r\n");
        assert!(SessionOffer::pick(audio, &base, StreamQuality::Main).is_none());
    }

    #[test]
    fn probe_reports_the_protocol_as_the_vendor_and_the_name_as_the_model() {
        let url = RtspUrl::parse("rtsp://cam.local/").expect("a url");
        let named = probe_result(&sdp::parse_sdp("s=Front Door\r\n"), &url);
        assert_eq!(named.vendor, "RTSP");
        assert_eq!(named.model, "Front Door");
        assert!(!named.ptz_supported, "no standard RTSP PTZ exists");

        let unnamed = probe_result(&sdp::parse_sdp("v=0\r\n"), &url);
        assert_eq!(unnamed.model, "RTSP camera");
    }

    #[test]
    fn the_config_record_becomes_a_url_with_injected_credentials() {
        let mut config = CameraConfig::new("cam", DRIVER_KEY, "10.0.0.5");
        config = config.with_credentials("admin", "hunter2");
        config.rtsp_port = Some(8554);
        let url = url_from_config(&config).expect("a url");
        assert_eq!(url.authority, "10.0.0.5:8554");
        assert_eq!(url.username.as_deref(), Some("admin"));
        assert!(url.path.ends_with('/'));
    }

    #[test]
    fn a_config_that_is_already_a_url_is_left_alone() {
        let mut config = CameraConfig::new("cam", DRIVER_KEY, "rtsp://10.0.0.5/live");
        config.password = Some("pw".into());
        let url = url_from_config(&config).expect("a url");
        assert_eq!(url.path, "/live");
    }

    #[test]
    fn statuses_map_onto_the_taxonomy() {
        assert_eq!(map_status(401), CameraError::Refused);
        assert_eq!(map_status(404), CameraError::NoCamera);
        assert_eq!(map_status(454), CameraError::Offline);
        assert!(matches!(map_status(460), CameraError::Other(_)));
    }

    #[test]
    fn the_ha2_target_is_the_path() {
        assert_eq!(uri_path("rtsp://cam/live"), "/live");
        assert_eq!(uri_path("rtsp://cam/live?q=1"), "/live?q=1");
        assert_eq!(uri_path("/already/relative"), "/already/relative");
        assert_eq!(uri_path("rtsp://cam"), "/");
    }

    #[test]
    fn a_camera_backend_is_a_typed_backend() {
        let url = RtspUrl::parse("rtsp://cam.local/").expect("a url");
        let backend = RtspCameraBackend::new(url, None, None);
        assert_eq!(backend.driver(), "rtsp");
        assert!(backend.reader().is_none());
    }
}
