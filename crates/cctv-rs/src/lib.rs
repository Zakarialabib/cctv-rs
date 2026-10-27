//! `cctv-rs` — the facade crate.
//!
//! The main entry point for host applications and edge clients. 
//! The primary facade crate the host application talks to. Nothing in here is
//! vendor-specific: the application sees only the `cctv-core` device model, the
//! reconciler's graded remediations, and the discovery sweep.
//!
//! - [`Facade`] — the single handle: register cameras, run commands, reconcile
//!   the fleet, sweep the LAN.
//! - [`DriverFactory`] — how a connection record's `driver` key becomes a live
//!   backend, so no vendor name is part of this API.
//! - `CameraError`, `CameraHandle`, `CommandId`, `CommandResult` — re-exported
//!   from `cctv-core`: the device model the application codes against.
//! - [`Remediation`], [`Grade`], [`Action`], [`Audit`] — re-exported from
//!   `cctv-agent`: what the reconciler decided, and why.

#![forbid(unsafe_code)]

pub mod drivers;
pub mod facade;

pub use drivers::{DriverFactory, EZVIZ, ONVIF, PTP, UVC};
pub use facade::{clock_ms, Facade};

pub use cctv_adapter_actioncam as actioncam;
pub use cctv_agent::{Action, Audit, AuditEntry, Grade, Reconciler, Remediation};
pub use cctv_core::{
    CameraConfig, CameraError, CameraHandle, CameraState, CameraStateCurrent, CameraType,
    CommandId, CommandResult, CommandResultPayload, DomainEvent, FrameSource, MediaFile, Preset,
    PtzCommand, Snapshot, StreamInfo, StreamQuality,
};
pub use cctv_stream_uvc::{
    BudgetedFrameReader, HalEventSource, OsError, OsPlatform, PtpAdapter, PtpStatus, UvcAdapter,
};
pub use cctv_discovery::DiscoveredCamera;
pub use cctv_protocol_ptp as ptp;
pub use cctv_storage_msc as msc;
