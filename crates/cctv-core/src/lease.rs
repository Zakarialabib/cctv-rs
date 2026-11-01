//! Maintenance lease domain model.
//!
//! A maintenance lease allows an external agent or operator to acquire
//! exclusive or shared maintenance access to a camera for a bounded duration.

use crate::prelude::*;

#[cfg(feature = "serde-sync")]
use serde::{Deserialize, Serialize};

/// Strongly typed identifier for a camera.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde-sync", serde(transparent))]
pub struct CameraId(pub String);

impl CameraId {
    /// Create a new `CameraId` from anything implementing `Into<String>`.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// View as string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl core::fmt::Display for CameraId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

impl core::ops::Deref for CameraId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<String> for CameraId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for CameraId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Mode of operation during an active maintenance lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde-sync", serde(rename_all = "snake_case"))]
pub enum LeaseMode {
    /// Observe camera traffic and relay status without interrupting operations.
    ObserveAndRelay,
    /// Disconnect normal consumers and yield exclusive access to the lease holder.
    DisconnectAndYield,
}

/// A maintenance lease held by a consumer for a specific camera.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct MaintenanceLease {
    /// Identifier of the entity holding the lease.
    pub holder_id: String,
    /// The camera covered by this lease.
    pub camera_id: CameraId,
    /// Unix timestamp in milliseconds when this lease expires.
    pub expires_at_unix_ms: u64,
    /// Operational mode of the lease.
    pub mode: LeaseMode,
}

impl MaintenanceLease {
    /// Create a new maintenance lease.
    pub fn new(
        holder_id: impl Into<String>,
        camera_id: impl Into<CameraId>,
        expires_at_unix_ms: u64,
        mode: LeaseMode,
    ) -> Self {
        Self {
            holder_id: holder_id.into(),
            camera_id: camera_id.into(),
            expires_at_unix_ms,
            mode,
        }
    }

    /// Check if the lease is currently active against the provided timestamp in unix milliseconds.
    pub fn is_active(&self, now_unix_ms: u64) -> bool {
        now_unix_ms < self.expires_at_unix_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camera_id_traits() {
        let id = CameraId::new("cam-123");
        assert_eq!(id.as_str(), "cam-123");
        assert_eq!(&*id, "cam-123");
        assert_eq!(id.to_string(), "cam-123");
        let id2: CameraId = "cam-123".into();
        assert_eq!(id, id2);
    }

    #[test]
    fn test_lease_is_active() {
        let lease = MaintenanceLease {
            holder_id: "operator-1".to_string(),
            camera_id: CameraId::new("cam-42"),
            expires_at_unix_ms: 10_000,
            mode: LeaseMode::ObserveAndRelay,
        };

        assert!(lease.is_active(9_999));
        assert!(!lease.is_active(10_000));
        assert!(!lease.is_active(10_001));
    }

    #[test]
    fn test_lease_modes() {
        let lease = MaintenanceLease {
            holder_id: "agent-x".to_string(),
            camera_id: CameraId::from("cam-99"),
            expires_at_unix_ms: 50_000,
            mode: LeaseMode::DisconnectAndYield,
        };
        assert_eq!(lease.mode, LeaseMode::DisconnectAndYield);
    }
}
