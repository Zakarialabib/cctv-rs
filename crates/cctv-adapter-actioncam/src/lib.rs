//! Action Camera adapter for cctv-rs.
//!
//! Provides passive mDNS TXT identification for devices like the Insta360 X4
//! and read-only status endpoint query capability.

#![forbid(unsafe_code)]

pub mod error;
pub mod mdns;
pub mod status;

pub use error::ActionCamError;
pub use mdns::{identify_insta360_x4, ActionCamIdentity};
pub use status::{ActionCamClient, ActionCamStatus};
