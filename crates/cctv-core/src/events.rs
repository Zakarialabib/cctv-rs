//! Domain events emitted by HAL / driver layer.

use crate::prelude::*;
#[cfg(feature = "serde-sync")]
use serde::{Deserialize, Serialize};

/// One recorded reconciliation audit entry.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct AuditEntry {
    /// The camera the remediation applied to.
    pub camera_id: String,
    /// What the reconciler did, in one line.
    pub what: String,
    /// The remediation's severity grade, as recorded.
    pub grade: u8,
    /// When it was applied, in Unix **milliseconds**.
    pub at_ms: u64,
}

/// Domain event emitted by hardware abstraction / drivers.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub enum DomainEvent {
    /// A new hardware camera node was attached / hot-plugged.
    DeviceAttached {
        /// Camera identity.
        camera_id: String,
        /// Driver or protocol name.
        driver: String,
    },
    /// A hardware camera node was dropped / unplugged.
    DeviceDropped {
        /// Camera identity.
        camera_id: String,
    },
}
