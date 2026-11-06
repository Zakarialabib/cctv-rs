//! Trust store and lockout / rate-limiting manager.
//!
//! Enforces:
//! - 5-minute timeout per pairing attempt
//! - 5 failed attempts -> 15-minute lockout

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// Standard ceremony timeout (5 minutes).
pub const CEREMONY_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Maximum failed attempts before lockout (5).
pub const MAX_FAILED_ATTEMPTS: u32 = 5;

/// Lockout duration following max failed attempts (15 minutes).
pub const LOCKOUT_DURATION: Duration = Duration::from_secs(15 * 60);

/// Trusted peer device entry in the trust store.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustedPeer {
    /// Device identifier (e.g., z-base-32 or string identifier).
    pub device_id: String,
    /// Friendly name of peer.
    pub name: String,
    /// Shared key or certificate fingerprint (zeroized on drop).
    #[serde(skip_serializing, skip_deserializing)]
    pub psk: Option<Zeroizing<Vec<u8>>>,
    /// UNIX timestamp when paired in milliseconds.
    pub paired_at_ms: u64,
}

impl fmt::Debug for TrustedPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrustedPeer")
            .field("device_id", &self.device_id)
            .field("name", &self.name)
            .field("psk", &self.psk.as_ref().map(|_| "***REDACTED***"))
            .field("paired_at_ms", &self.paired_at_ms)
            .finish()
    }
}

/// In-memory / persistable trust store holding trusted peers and pairing nonce counter persistence.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TrustStore {
    peers: HashMap<String, TrustedPeer>,
    /// Persisted last seen strictly monotonic nonce counter per device identifier.
    #[serde(default)]
    nonce_counters: HashMap<String, u64>,
}

impl TrustStore {
    /// Create a new empty trust store.
    pub fn new() -> Self {
        Self {
            peers: HashMap::new(),
            nonce_counters: HashMap::new(),
        }
    }

    /// Add or update a trusted peer.
    pub fn insert(&mut self, peer: TrustedPeer) {
        self.peers.insert(peer.device_id.clone(), peer);
    }

    /// Retrieve a trusted peer by device ID.
    pub fn get(&self, device_id: &str) -> Option<&TrustedPeer> {
        self.peers.get(device_id)
    }

    /// Check if a device ID is trusted.
    pub fn contains(&self, device_id: &str) -> bool {
        self.peers.contains_key(device_id)
    }

    /// Remove a trusted peer.
    pub fn remove(&mut self, device_id: &str) -> Option<TrustedPeer> {
        self.peers.remove(device_id)
    }

    /// Number of trusted devices.
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Whether store is empty.
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Iterate over trusted peers.
    pub fn iter(&self) -> impl Iterator<Item = &TrustedPeer> {
        self.peers.values()
    }

    /// Get the highest recorded nonce counter for a device ID.
    pub fn get_nonce_counter(&self, device_id: &str) -> u64 {
        self.nonce_counters.get(device_id).copied().unwrap_or(0)
    }

    /// Validates and records a strictly monotonic nonce counter for a device.
    ///
    /// Returns `true` if the counter is strictly greater than the recorded counter,
    /// updating the persisted counter; returns `false` if replayed or non-monotonic.
    pub fn check_and_advance_nonce(&mut self, device_id: &str, counter: u64) -> bool {
        let current = self.get_nonce_counter(device_id);
        if counter > current {
            self.nonce_counters.insert(device_id.to_string(), counter);
            true
        } else {
            false
        }
    }

    /// Sets the persisted nonce counter for a device ID directly.
    pub fn set_nonce_counter(&mut self, device_id: &str, counter: u64) {
        self.nonce_counters.insert(device_id.to_string(), counter);
    }
}

/// Tracks failed pairing attempts and enforces lockout intervals.
#[derive(Debug, Clone, Default)]
pub struct LockoutManager {
    /// Consecutive failed attempt counter per remote identifier / IP.
    failed_attempts: HashMap<String, u32>,
    /// Active lockout expiration timestamps in milliseconds (`now_ms + 15 min`).
    lockouts: HashMap<String, u64>,
}

impl LockoutManager {
    /// Create a new lockout manager.
    pub fn new() -> Self {
        Self {
            failed_attempts: HashMap::new(),
            lockouts: HashMap::new(),
        }
    }

    /// Check if a peer or identity is currently locked out.
    pub fn is_locked_out(&self, id: &str, now_ms: u64) -> bool {
        if let Some(&locked_until) = self.lockouts.get(id) {
            now_ms < locked_until
        } else {
            false
        }
    }

    /// Remaining lockout duration in milliseconds, or 0 if not locked out.
    pub fn remaining_lockout_ms(&self, id: &str, now_ms: u64) -> u64 {
        if let Some(&locked_until) = self.lockouts.get(id) {
            locked_until.saturating_sub(now_ms)
        } else {
            0
        }
    }

    /// Record a successful attempt, resetting failure counter and clearing lockout.
    pub fn record_success(&mut self, id: &str) {
        self.failed_attempts.remove(id);
        self.lockouts.remove(id);
    }

    /// Record a failed attempt. If failures reach `MAX_FAILED_ATTEMPTS` (5),
    /// enters a 15-minute lockout (`now_ms + LOCKOUT_DURATION`).
    /// Returns `true` if this failure triggered a lockout.
    pub fn record_failure(&mut self, id: &str, now_ms: u64) -> bool {
        // If already locked out and still in lockout period, keep lockout
        if self.is_locked_out(id, now_ms) {
            return true;
        }

        // If a previous lockout expired, reset attempts
        if let Some(&locked_until) = self.lockouts.get(id) {
            if now_ms >= locked_until {
                self.lockouts.remove(id);
                self.failed_attempts.remove(id);
            }
        }

        let count = self.failed_attempts.entry(id.to_string()).or_insert(0);
        *count += 1;

        if *count >= MAX_FAILED_ATTEMPTS {
            let lock_until = now_ms.saturating_add(LOCKOUT_DURATION.as_millis() as u64);
            self.lockouts.insert(id.to_string(), lock_until);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trust_store() {
        let mut store = TrustStore::new();
        assert!(store.is_empty());

        let peer = TrustedPeer {
            device_id: "dev-1".to_string(),
            name: "Living Room Camera".to_string(),
            psk: Some(Zeroizing::new(vec![1, 2, 3])),
            paired_at_ms: 1000,
        };

        store.insert(peer.clone());
        assert_eq!(store.len(), 1);
        assert!(store.contains("dev-1"));
        assert_eq!(store.get("dev-1").unwrap().name, "Living Room Camera");

        // Nonce counter persistence test
        assert_eq!(store.get_nonce_counter("dev-1"), 0);
        assert!(store.check_and_advance_nonce("dev-1", 10));
        assert_eq!(store.get_nonce_counter("dev-1"), 10);
        // Non-monotonic or duplicate counter is rejected
        assert!(!store.check_and_advance_nonce("dev-1", 10));
        assert!(!store.check_and_advance_nonce("dev-1", 9));
        // Strictly monotonic counter advances
        assert!(store.check_and_advance_nonce("dev-1", 11));
        assert_eq!(store.get_nonce_counter("dev-1"), 11);

        let removed = store.remove("dev-1");
        assert!(removed.is_some());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn test_lockout_after_5_failures() {
        let mut manager = LockoutManager::new();
        let id = "192.168.1.100";
        let mut now_ms = 1_000_000;

        assert!(!manager.is_locked_out(id, now_ms));

        for i in 1..5 {
            let locked = manager.record_failure(id, now_ms);
            assert!(!locked, "should not lock at failure {i}");
            assert!(!manager.is_locked_out(id, now_ms));
        }

        // 5th failure triggers lockout
        let locked = manager.record_failure(id, now_ms);
        assert!(locked);
        assert!(manager.is_locked_out(id, now_ms));

        // 14 minutes later: still locked out
        now_ms += 14 * 60 * 1000;
        assert!(manager.is_locked_out(id, now_ms));
        assert!(manager.remaining_lockout_ms(id, now_ms) > 0);

        // 15 minutes + 1 ms: lockout expired
        now_ms += 60 * 1000 + 1;
        assert!(!manager.is_locked_out(id, now_ms));
        assert_eq!(manager.remaining_lockout_ms(id, now_ms), 0);
    }
}
