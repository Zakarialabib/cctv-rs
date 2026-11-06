//! RTP header parsing, H.264 / H.265 depacketisation, and Annex-B stream probing for cctv-rs.
//!
//! **The Payload.**  A camera does not send frames; it sends RTP packets.  One
//! frame may arrive as a single NAL unit, as an aggregation of several, or split
//! across many fragments, and the only thing marking the end is the RTP marker
//! bit.  This crate puts them back together.
//!
//! It is **standalone and dependency-free** — byte slices in, no socket, no
//! runtime, no codec library.  The session-level pieces (RTSP framing, SDP) live
//! in `cctv-rtsp`; this crate knows nothing about them, which is why it can be
//! tested exhaustively and reused by any transport.
//!
//! - [`rtp`] — the 12-byte header, with CSRC lists, extensions and padding handled.
//! - [`h264`] — access units from single NALs, STAP-A and FU-A (RFC 6184).
//! - [`h265`] — access units from single NALs, AP and FU (RFC 7798).
//! - [`annexb`] — Annex-B elementary stream parser and parameter extractor.
//! - [`probe`] — [`MediaProbe`], [`MediaDescriptor`], and [`TranscodePlan`].
//! - [`types`] — [`AccessUnit`], the shape both depacketisers produce.
//! - [`error`] — [`RtpError`], including [`RtpError::Incomplete`].
//!
//! Both depacketisers drop a broken unit rather than emit it: a decoder handed
//! half a frame produces garbage, which is worse than a dropped frame.

#![forbid(unsafe_code)]

pub mod annexb;
pub mod error;
pub mod h264;
pub mod h265;
pub mod probe;
pub mod rtp;
pub mod types;

pub use annexb::{
    parse_h264_sps, parse_h265_sps, probe_annexb, split_annexb, unescape_rbsp, BitReader,
    StreamParameters, VideoCodec,
};
pub use error::RtpError;
pub use h264::H264Depacketizer;
pub use h265::{parse_nal_header, H265Depacketizer, NalHeader};
pub use probe::{MediaDescriptor, MediaProbe, TranscodePlan};
pub use rtp::{parse_rtp, RtpPacket};
pub use types::AccessUnit;
