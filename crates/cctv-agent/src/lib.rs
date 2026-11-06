//! Fleet reconciler and edge client streaming services.
//!
//! Holds the desired state, compares it against the observed state (via
//! `cctv-core`), and applies graded remediation.  Audits every change so a
//! misconfigured fleet can be rewound.
//!
//! In addition, provides:
//! - [`stream_mux`] — Single-pull sub-stream RTSP demuxing, NALU parsing, GOP caching
//!   `[SPS, PPS, (VPS), IDR_Keyframe]`, and zero-GOP wait burst delivery over broadcast channels.
//! - [`snapshot_proxy`] — Native HTTP snapshot caching proxy with Digest authentication.
//!
//! - [`Reconciler`] — `new`, `set_desired`, `record_error`, `reconcile`,
//!   `reconcile_all`.
//! - [`Remediation`] — one graded decision: what it does, and how invasive it is.
//! - [`Grade`] / [`Action`] — the grading scale and the state change it implies.
//! - [`Audit`] / [`AuditEntry`] — the append-only record.

#![forbid(unsafe_code)]

pub mod audit;
pub mod events;
pub mod ptz_watchdog;
pub mod reconcile;
pub mod remedy;
pub mod runner;
pub mod session_budget;
pub mod snapshot_proxy;
pub mod stream_mux;

pub use audit::{Audit, AuditEntry};
pub use cctv_core::{
    CameraCapabilitySet, CameraId, CameraProvisioningBundleV1, DecryptedCameraAuth,
    ImperativeCameraCommand, LeaseMode, MaintenanceLease, StreamProfileSet,
};
pub use events::{DetectionEvent, EventSubscriptionService, MotionEventStream, SignageCommand};
pub use ptz_watchdog::{PtzPulse, PtzStop, PtzWatchdog};
pub use reconcile::{Action, CommandRelay, Grade, Reconciler, Remediation};
pub use remedy::{ClockHealing, ClockSyncDevice, RemedyStep, MAX_CLOCK_OFFSET_MS};
pub use runner::{
    AgentRunner, InMemoryJsonlAuditLog, JsonlAuditEntry, JsonlAuditLog, OnvifPtzDispatcher,
    RunnerCommand,
};
pub use session_budget::{CameraCapabilities, SessionManager};
pub use snapshot_proxy::{CachedSnapshot, SnapshotProxy, SnapshotProxyConfig};
pub use stream_mux::{
    classify_nalu, GopCache, Nalu, NaluType, StreamMux, StreamMuxConfig, StreamSubscription,
    VideoCodec,
};
