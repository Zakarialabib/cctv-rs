//! Fleet reconciler.
//!
//! Holds the desired state, compares it against the observed state (via
//! `cctv-core`), and applies graded remediation. Audits every change so we can
//! rewind a misconfigured fleet.

pub mod reconcile;
pub mod audit;

pub use audit::*;
pub use reconcile::*;
