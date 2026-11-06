//! State synchronization for CCTV trust store and camera device registry.
//!
//! Provides conflict-free replicated data types (CRDTs) allowing edge nodes,
//! workstations, and video management displays to converge state reliably
//! across network partitions and offline pairing ceremonies.

#![forbid(unsafe_code)]

pub mod crdt;
pub mod registry;
pub mod trust;

pub use crdt::{LwwRegister, OrSet, Tag};
pub use registry::{SyncCameraRecord, SyncDeviceRegistry};
pub use trust::{SyncPeerRecord, SyncTrustStore};
