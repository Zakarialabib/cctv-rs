//! TLS policy + budgeted frame reader.
//!
//! The single place a TLS stack enters. Policy is data (pinned certs, min TLS
//! version); frame reading is budgeted so a slow camera never stalls the fleet.
//!
//! The driver crates also share two wire-level helpers from here:
//!
//! - [`xml`] — a total, namespace-prefix-agnostic XML scanner (always on).
//! - [`digest`] — HTTP Digest challenge parsing and the `Authorization` header
//!   (always on): pure hashing that both the HTTP client and the RTSP session
//!   answer a `401` with, so it must not be gated behind a client it does not
//!   need.
//! - `http` (feature `http`) — the async HTTP client, `reqwest` + **rustls
//!   only**, which imports [`digest`] rather than owning the algorithm.
//! - `net` (feature `net`) — the raw socket layer this crate is for: TCP, UDP
//!   and TLS with [`policy`] applied, plus the byte reader that feeds
//!   [`reader`].  No `reqwest`; it is what eventually replaces `http`.
//! - `fixture` (feature `fixture`) — a canned-response HTTP server the
//!   drivers' conformance tests run against.
//!
//! TLS policy is injected, not compiled — the reconciler can swap the loader at
//! runtime for different deployments (VPS mirror, office player, kiosk).

#![forbid(unsafe_code)]

pub mod digest;
#[cfg(feature = "fixture")]
pub mod fixture;
#[cfg(feature = "http")]
pub mod http;
#[cfg(feature = "net")]
pub mod net;
pub mod policy;
pub mod reader;
pub mod xml;
