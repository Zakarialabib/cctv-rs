//! RTSP / RTP / H.264 codecs for cctv-rs.
//!
//! The protocol half of the stream path, and the half where bugs live: an RTSP
//! session is a handful of requests, but the frames arrive as RTP packets that
//! have to be reassembled, and a camera will happily hand you a fragmented NAL
//! split across three packets and a marker bit on the last one.
//!
//! This crate is **pure**: it works on byte slices and never opens a socket, so
//! every rule below is unit-tested without a camera, a server or a runtime.  The
//! socket loop that drives it is the driver's job.
//!
//! - [`message`] — RTSP request / response framing, and the `$`-prefixed
//!   interleaved frames a TCP transport carries in the same connection.
//! - [`sdp`] — the session description `DESCRIBE` returns: which track, which
//!   payload type, which codec, and the out-of-band parameter sets.
//! - [`rtp`] — the 12-byte RTP header and its payload.
//! - [`h264`] — reassembly of single NAL units, STAP-A aggregates and FU-A
//!   fragments into the access units a decoder can take.
//! - [`error`] — [`RtspError`], including [`RtspError::Incomplete`], which is not
//!   a failure: it means "read more bytes and try again".

#![forbid(unsafe_code)]

pub mod error;
pub mod h264;
pub mod message;
pub mod rtp;
pub mod sdp;

pub use error::RtspError;
pub use h264::{AccessUnit, H264Depacketizer};
pub use message::{Headers, Message, Method, Request, Response};
pub use rtp::{parse_rtp, RtpPacket};
pub use sdp::{parse_sdp, Session, Track};
