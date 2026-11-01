//! Fleet reconciler.
//!
//! Holds the desired state, compares it against the observed state (via
//! `cctv-core`), and applies graded remediation.  Audits every change so a
//! misconfigured fleet can be rewound.
//!
//! - [`Reconciler`] — `new`, `set_desired`, `record_error`, `reconcile`,
//!   `reconcile_all`.
//! - [`Remediation`] — one graded decision: what it does, and how invasive it is.
//! - [`Grade`] / [`Action`] — the grading scale and the state change it implies.
//! - [`Audit`] / [`AuditEntry`] — the append-only record.

#![forbid(unsafe_code)]

pub mod audit;
pub mod ptz_watchdog;
pub mod reconcile;
pub mod remedy;
pub mod runner;
pub mod session_budget;

pub use audit::{Audit, AuditEntry};
pub use ptz_watchdog::{PtzPulse, PtzStop, PtzWatchdog};
pub use reconcile::{Action, CommandRelay, Grade, Reconciler, Remediation};
pub use remedy::{ClockHealing, ClockSyncDevice, RemedyStep, MAX_CLOCK_OFFSET_MS};
pub use runner::{
    AgentRunner, InMemoryJsonlAuditLog, JsonlAuditEntry, JsonlAuditLog, OnvifPtzDispatcher,
    RunnerCommand,
};
pub use session_budget::{CameraCapabilities, CameraId, MaintenanceLease, SessionManager};
