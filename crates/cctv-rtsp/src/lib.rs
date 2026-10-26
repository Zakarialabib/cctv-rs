//! RTSP session codecs for cctv-rs.
//!
//! The session half of the stream path: how a camera is *asked* for a stream,
//! and what it answers with.  The payload half — RTP headers and H.264 / H.265
//! reassembly — is `cctv-rtp`, a separate crate, because a payload parser has
//! nothing to do with a session and deserves to be testable on its own.
//!
//! This crate is **pure**: it works on byte slices and never opens a socket, so
//! every rule is unit-tested without a camera, a server or a runtime.
//!
//! - [`message`] — RTSP request / response framing, and the `$`-prefixed
//!   interleaved frames a TCP transport carries on the same connection.
//! - [`sdp`] — the session description `DESCRIBE` returns: which track, which
//!   payload type, which codec, and the out-of-band parameter sets.
//! - [`error`] — [`RtspError`], including [`RtspError::Incomplete`], which is not
//!   a failure: it means "read more bytes and try again".
//!
//! The `CameraBackend` implementation — DESCRIBE → SETUP → PLAY over a socket,
//! Digest auth, interleaved demux, keepalive — is the next step and belongs
//! here once `cctv-transport` grows a socket layer.

#![forbid(unsafe_code)]

pub mod error;
pub mod message;
pub mod sdp;

pub use error::RtspError;
pub use message::{Headers, Message, Method, Request, Response};
pub use sdp::{parse_sdp, Session, Track};
