//! RTSP session codecs for cctv-rs.
//!
//! The session half of the stream path: how a camera is *asked* for a stream,
//! and what it answers with.  The payload half — RTP headers and H.264 / H.265
//! reassembly — is `cctv-rtp`, a separate crate, because a payload parser has
//! nothing to do with a session and deserves to be testable on its own.
//!
//! Two halves, deliberately separated:
//!
//! - **Pure** (always compiled): [`message`] and [`sdp`] work on byte slices and
//!   never open a socket, so every rule is unit-tested without a camera, a
//!   server or a runtime.
//! - **Live** (feature `net`): `backend` speaks RTSP over `cctv-transport`'s
//!   socket layer — DESCRIBE → SETUP → PLAY, Digest auth, and the
//!   `streaming` demux that turns interleaved RTP into access units for the
//!   facade's `FrameReader`.  Gated because a caller who only wants the parsers
//!   must not link `tokio`.
//!
//! - [`error`] — [`RtspError`], including [`RtspError::Incomplete`], which is not
//!   a failure: it means "read more bytes and try again".

#![forbid(unsafe_code)]

#[cfg(feature = "net")]
pub mod backend;
pub mod error;
pub mod message;
pub mod sdp;
#[cfg(feature = "net")]
pub mod streaming;

pub use error::RtspError;
pub use message::{Headers, Message, Method, Request, Response};
pub use sdp::{parse_sdp, Session, Track};

#[cfg(feature = "net")]
pub use backend::{
    map_net_error, map_status, probe_result, resolve_control, uri_path, RtspCameraBackend,
    RtspSession, RtspUrl, SessionOffer, DEFAULT_KEEPALIVE, DEFAULT_RECONNECT_DELAY,
    DEFAULT_RETRIES, DRIVER_KEY, GENERIC_MODEL, GENERIC_VENDOR,
};
#[cfg(feature = "net")]
pub use streaming::{demux, run_once, Depacketizer, Frame, StreamHandle, StreamOutcome, READ_CHUNK};
