//! TLS policy + budgeted frame reader.
//!
//! The single place a TLS stack enters. Policy is data (pinned certs, min TLS
//! version); frame reading is budgeted so a slow camera never stalls the fleet.
//!
//! The driver crates also share two wire-level helpers from here:
//!
//! - [`xml`] — a total, namespace-prefix-agnostic XML scanner (always on).
//! - `http` (feature `http`) — the async HTTP client, `reqwest` + **rustls
//!   only**, with HTTP Digest / Basic negotiation.
//! - `fixture` (feature `fixture`) — a canned-response HTTP server the
//!   drivers' conformance tests run against.
//!
//! TLS policy is injected, not compiled — the reconciler can swap the loader at
//! runtime for different deployments (VPS mirror, office player, kiosk).

#![forbid(unsafe_code)]

#[cfg(feature = "fixture")]
pub mod fixture;
#[cfg(feature = "http")]
pub mod http;
pub mod policy;
pub mod reader;
pub mod xml;
