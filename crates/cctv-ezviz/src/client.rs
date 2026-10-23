//! Ezviz client facade.
//!
//! The single place the driver knows how the Ezviz Open API is wired.  Public
//! operations are expressed in cctv device-model terms, not Ezviz terms.

pub use crate::types::{Connectivity, EzvizCamera};
pub use ezviz_openapi::EzvizClient;
