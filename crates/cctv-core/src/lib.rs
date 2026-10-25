//! cctv-core — the camera device model (no tokio, no protocol).
//!
//! Everything a frontend or a reconciler needs to reason about a camera without
//! knowing *who* makes it.  Protocols arrive through the driver crates.
//!
//! # Design
//!
//! `cctv-core` is the **domain** (the aggregate): `CameraState`, `CameraDesired`,
//! `CameraStateCurrent`, `CameraType`, `ProbeResult`, `CommandId` and the
//! `CommandResult` / `CommandResultPayload` pair.  The drivers
//! (`cctv-ezviz`, `cctv-discovery`, `cctv-transport`) are pure **ports**: they
//! translate hardware into this aggregate and nothing else.
//!
//! Drivers implement [`CameraBackend`] for one connected camera and prove it
//! with the [`conformance`] suite.  The trait is object safe and returns
//! `std`-only boxed futures, so the contract still links no async runtime.
//!
//! # Example
//!
//! ```
//! use cctv_core::{redact_uri, CameraConfig, PtzCommand};
//!
//! let cfg = CameraConfig::new("lobby", "onvif", "192.168.1.64").with_credentials("admin", "pw");
//! assert_eq!(cfg.base_url(80), "http://192.168.1.64");
//! assert_eq!(PtzCommand::from_direction("stop", 1.0), PtzCommand::Stop);
//! assert_eq!(redact_uri("rtsp://admin:pw@cam/s"), "rtsp://***:***@cam/s");
//! ```
//!
//! The crate carries no runtime and no network — `serde` / `serde_json` are only
//! used for the command-result value types.  That makes the aggregate embeddable
//! in a Tauri app, a terminal CLI, or even a WASM worker.

#![forbid(unsafe_code)]

pub mod backend;
pub mod commands;
pub mod conformance;
pub mod error;
pub mod mock;
#[cfg(feature = "quirks")]
pub mod quirks;
pub mod state;
pub mod types;

// ---------------------------------------------------------------------------
// Domain exports (the aggregate surface).
//
// Naming rule — the aggregate uses `Camera*` for its nouns
// (`CameraState`, `CameraDesired`, `CameraStateCurrent`), plain nouns for value
// objects (`ProbeResult`, `CameraSnapshot`, `CommandResult`, `StreamHandle`),
// and `CameraError` for the failure type.  The domain is the contract; drivers
// never widen it.
// ---------------------------------------------------------------------------

pub use backend::{
    jpeg_dimensions, redact_uri, with_credentials, BoxFuture, CameraBackend, CameraConfig,
    PtzCommand, Snapshot, StreamInfo, StreamQuality,
};
pub use commands::{
    CameraHandle, CameraSnapshot, CommandId, CommandResult, CommandResultPayload, MotionEvent,
    Preset, SnapshotRef, StreamHandle,
};
pub use error::CameraError;
pub use state::{CameraDesired, CameraState, CameraStateCurrent, ProbeResult};
pub use types::CameraType;
