//! Replicated Trust Store synchronized using CRDTs.

use std::collections::HashMap;

use cctv_pair::trust::{TrustStore, TrustedPeer};
use serde::{Deserialize, Serialize};

use crate::crdt::{LwwRegister, OrSet, Tag};

/// Replicated peer record with CRDT conflict resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPeerRecord {
    /// Unique device ID.
    pub device_id: String,
    /// Device friendly name (LWW Register).
    pub name: LwwRegister<String>,
    /// Pairing timestamp in Unix milliseconds.
    pub paired_at_ms: u64,
    /// Public certificate fingerprint or PSK commitment hash if present.
    pub psk_fingerprint: Option<String>,
}

/// A replicated trust store synchronizing across edge devices and workspaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncTrustStore {
    /// Local node ID.
    pub node_id: String,
    /// Sequence counter for tag generation.
    seq: u64,
    /// Set of active device IDs (OR-Set).
    device_set: OrSet<String>,
    /// Metadata registers for peers.
    peers: HashMap<String, SyncPeerRecord>,
}

impl SyncTrustStore {
    /// Create a new replicated trust store for `node_id`.
    pub fn new(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            seq: 0,
            device_set: OrSet::new(),
            peers: HashMap::new(),
        }
    }

    fn next_tag(&mut self, now_ms: u64) -> Tag {
        self.seq += 1;
        Tag {
            timestamp_ms: now_ms,
            node_id: self.node_id.clone(),
            seq: self.seq,
        }
    }

    /// Add or update a trusted peer.
    pub fn insert_peer(&mut self, peer: TrustedPeer, now_ms: u64) {
        let id = peer.device_id.clone();
        let tag = self.next_tag(now_ms);

        self.device_set.add(id.clone(), tag);

        let record = self
            .peers
            .entry(id.clone())
            .or_insert_with(|| SyncPeerRecord {
                device_id: id.clone(),
                name: LwwRegister::new(peer.name.clone(), now_ms, &self.node_id),
                paired_at_ms: peer.paired_at_ms,
                psk_fingerprint: None,
            });

        record.name.update(peer.name, now_ms, &self.node_id);
    }

    /// Revoke/remove a trusted peer.
    pub fn revoke_peer(&mut self, device_id: &str) -> bool {
        self.device_set.remove(&device_id.to_string())
    }

    /// Check if a device ID is trusted.
    pub fn contains(&self, device_id: &str) -> bool {
        self.device_set.contains(&device_id.to_string())
    }

    /// Retrieve peer metadata.
    pub fn get(&self, device_id: &str) -> Option<TrustedPeer> {
        if !self.contains(device_id) {
            return None;
        }
        self.peers.get(device_id).map(|rec| TrustedPeer {
            device_id: rec.device_id.clone(),
            name: rec.name.value.clone(),
            psk: None,
            paired_at_ms: rec.paired_at_ms,
        })
    }

    /// List all trusted peers.
    pub fn list_peers(&self) -> Vec<TrustedPeer> {
        self.device_set
            .elements()
            .into_iter()
            .filter_map(|id| self.get(&id))
            .collect()
    }

    /// Convert to a standard `cctv_pair::trust::TrustStore`.
    pub fn to_trust_store(&self) -> TrustStore {
        let mut store = TrustStore::new();
        for peer in self.list_peers() {
            store.insert(peer);
        }
        store
    }

    /// Ingest a standard `TrustStore` into this sync store.
    pub fn ingest_trust_store(&mut self, store: &TrustStore, now_ms: u64) {
        for peer in store.iter() {
            self.insert_peer(peer.clone(), now_ms);
        }
    }

    /// Merge state from a remote `SyncTrustStore`.
    pub fn merge(&mut self, other: &Self) {
        self.device_set.merge(&other.device_set);

        for (id, other_rec) in &other.peers {
            match self.peers.get_mut(id) {
                Some(local_rec) => {
                    local_rec.name.merge(&other_rec.name);
                    if other_rec.paired_at_ms < local_rec.paired_at_ms {
                        local_rec.paired_at_ms = other_rec.paired_at_ms;
                    }
                }
                None => {
                    self.peers.insert(id.clone(), other_rec.clone());
                }
            }
        }
    }
}
