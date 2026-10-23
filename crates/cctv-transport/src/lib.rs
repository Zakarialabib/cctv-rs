//! TLS policy + budgeted frame reader.
//!
//! The single place a TLS stack enters. Policy is data (pinned certs, min TLS
//! version); frame reading is budgeted so a slow camera never stalls the fleet.
//!
//! TLS policy is injected, not compiled — the reconciler can swap the loader at
//! runtime for different deployments (VPS mirror, office player, kiosk).

#![forbid(unsafe_code)]

pub mod policy;
pub mod reader;
