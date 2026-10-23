//! Fleet reconciler.
//!
//! Holds the desired state, compares it against the observed state (via
//! `cctv-core`), and applies graded remediation. Audits every change so we can
//! rewind a misconfigured fleet.

#![forbid(unsafe_code)]

pub mod audit;
pub mod reconcile;

pub use audit::*;
pub use reconcile::*;
