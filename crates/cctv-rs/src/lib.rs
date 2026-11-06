//! `cctv-rs` — the facade crate.
//!
//! The main entry point for host applications and edge clients.  Nothing in here is
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

pub use cctv_agent::{Action, Audit, AuditEntry, CommandRelay, Grade, Reconciler, Remediation};
pub use cctv_core::{
    BackendCapabilityRegistry, CameraBackend, CameraConfig, CameraError, CameraHandle, CameraId,
    CameraState, CameraStateCurrent, CameraType, Capability, CapabilitySet, CapabilityView,
    CommandId, CommandResult, CommandResultPayload, DomainEvent, FrameSource, MediaFile, Preset,
    PtzCommand, Snapshot, StreamInfo, StreamQuality,
};
pub use cctv_detect::{Detection, Region, RegionKind, Zone, ZoneConfig};
pub use cctv_discovery::DiscoveredCamera;
pub use cctv_pair::{
    CeremonyConfig, CeremonyRole, CeremonyState, LockoutManager, PairingCeremony, PairingError,
    PairingMessage, PairingMethod, PairingQrPayload, TrustStore, TrustedPeer, PAIRING_ALPN,
};
pub use cctv_rag::{
    AdjacentToEdge, Bm25Index, CameraKind, CameraNode, CoversEdge, DiagnosticPlaybook,
    EmbeddingProvider, HandoffCandidate, IncidentChunk, IncidentCoalescer, IncidentSeverity,
    IncidentTelemetry, KnowledgeBaseConfig, KnowledgeBaseIndex, KnowledgeChunk, PlaybookRegistry,
    PresetNode, PtzReachesEdge, RagContextEnvelope, RagError, ScoredResult, SearchDocument,
    SpatialGraph, ZoneKind, ZoneNode,
};
pub use cctv_rtp::{MediaDescriptor, MediaProbe, TranscodePlan};
pub use cctv_stream_uvc::{
    BudgetedFrameReader, HalEventSource, OsError, OsPlatform, PtpAdapter, PtpStatus, UvcAdapter,
};

#[cfg(feature = "ddc")]
pub use cctv_agent::{AmbientDisplayPolicy, NIGHT_LUX_THRESHOLD};
#[cfg(feature = "ddc")]
pub use cctv_stream_uvc::{
    DdcController, DdcError, DisplayProfile, I2cBus, InputSource, LinuxI2cBus, MockI2cBus,
    PowerMode, VcpCode, DDC_HOST_ADDR, DDC_I2C_ADDR,
};
