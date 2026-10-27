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
//! with the [`conformance`] suite.  The trait is object safe and returns boxed
//! futures, so the contract links no async runtime.
//!
//! # `no_std`
//!
//! The contract is **`alloc`-only**: no `std`, no runtime, no protocol, and no
//! JSON DOM.  Two things need `std` and are therefore features rather than part
//! of the contract:
//!
//! - `testing` — the mock backend.  It needs `Mutex` for interior mutability,
//!   because a [`CameraBackend`] must be `Sync`, and `alloc` has no mutex.
//! - `quirks` — the model/firmware table, because `toml` is `std`-only.
//!
//! `default = ["testing"]`, so `cargo check -p cctv-core --no-default-features`
//! is the alloc-only build and that is the gate that keeps it honest.
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
//! The crate carries no runtime and no network — `serde` is only used for the
//! command-result value types.  That makes the aggregate embeddable in a Tauri
//! app, a terminal CLI, or even a WASM worker.

#![cfg_attr(not(any(feature = "testing", feature = "quirks")), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

#[cfg(test)]
extern crate std;

/// The `alloc` items the crate's modules use, in one glob import.
///
/// Glob imports never warn when unused, so every module can take this
/// unconditionally — which is cheaper than tracking which module needs `vec!`.
///
/// The re-exports themselves are only *needed* in the alloc-only build: with
/// `std` linked the std prelude supplies the same names, so they read as unused.
#[allow(unused_imports)]
pub(crate) mod prelude {
    pub use alloc::borrow::ToOwned;
    pub use alloc::boxed::Box;
    pub use alloc::format;
    pub use alloc::string::{String, ToString};
    pub use alloc::vec;
    pub use alloc::vec::Vec;
}

pub mod backend;
pub mod commands;
pub mod conformance;
pub mod error;
#[cfg(feature = "testing")]
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
    jpeg_dimensions, redact_uri, with_credentials, CameraBackend, CameraConfig, PtzCommand,
    Snapshot, StreamInfo, StreamQuality,
};
pub use commands::{
    CameraHandle, CameraSnapshot, CommandId, CommandResult, CommandResultPayload, MotionEvent,
    Preset, SnapshotRef, StreamHandle,
};
pub use error::CameraError;
pub use state::{CameraDesired, CameraState, CameraStateCurrent, ProbeResult};
pub use types::CameraType;
