//! Domain events emitted by HAL / driver layer.

use crate::prelude::*;
use serde::{Deserialize, Serialize};

/// Domain event emitted by hardware abstraction / drivers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
