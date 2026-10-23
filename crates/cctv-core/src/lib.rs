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
//! The crate carries no runtime and no network — `serde` / `serde_json` are only
//! used for the command-result value types.  That makes the aggregate embeddable
//! in a Tauri app, a terminal CLI, or even a WASM worker.

#![forbid(unsafe_code)]

pub mod commands;
pub mod error;
pub mod mock;
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

pub use commands::{
    CameraHandle, CameraSnapshot, CommandId, CommandResult, CommandResultPayload, MotionEvent,
    Preset, SnapshotRef, StreamHandle,
};
pub use error::CameraError;
pub use state::{CameraDesired, CameraState, CameraStateCurrent, ProbeResult};
pub use types::CameraType;
