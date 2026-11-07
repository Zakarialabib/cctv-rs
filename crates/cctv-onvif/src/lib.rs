//! Deprecated re-export shim for `cctv-onvif`.
//!
//! The ONVIF driver has folded into [`cctv_driver::onvif`].
//! This crate remains as a backward-compatibility re-export shim for one release.

#![deprecated(
    since = "0.3.0",
    note = "cctv-onvif has folded into cctv-driver::onvif; migrate to cctv-driver with features = [\"onvif\"]"
)]
#![allow(deprecated)]

pub use cctv_driver::onvif::*;

/// ONVIF authentication utilities re-exported from `cctv_driver::onvif::auth`.
pub mod auth {
    pub use cctv_driver::onvif::auth::*;
}

/// ONVIF camera backend re-exported from `cctv_driver::onvif::backend`.
pub mod backend {
    pub use cctv_driver::onvif::backend::*;
}

/// ONVIF device service re-exported from `cctv_driver::onvif::device`.
pub mod device {
    pub use cctv_driver::onvif::device::*;
}

/// ONVIF discovery delegate re-exported from `cctv_driver::onvif::discovery`.
pub mod discovery {
    pub use cctv_driver::onvif::discovery::*;
}

/// ONVIF errors re-exported from `cctv_driver::onvif::error`.
pub mod error {
    pub use cctv_driver::onvif::error::*;
}

/// ONVIF WS-Eventing pull-point subscriptions re-exported from `cctv_driver::onvif::events`.
pub mod events {
    pub use cctv_driver::onvif::events::*;
}

/// ONVIF media service re-exported from `cctv_driver::onvif::media`.
pub mod media {
    pub use cctv_driver::onvif::media::*;
}

/// Observation bridge re-exported from `cctv_driver::onvif::observe_bridge`.
pub mod observe_bridge {
    pub use cctv_driver::onvif::observe_bridge::*;
}

/// ONVIF PTZ service re-exported from `cctv_driver::onvif::ptz`.
pub mod ptz {
    pub use cctv_driver::onvif::ptz::*;
}

/// ONVIF session re-exported from `cctv_driver::onvif::session`.
pub mod session {
    pub use cctv_driver::onvif::session::*;
}

/// ONVIF SOAP envelope and header builders re-exported from `cctv_driver::onvif::soap`.
pub mod soap {
    pub use cctv_driver::onvif::soap::*;
}
