//! Conflict-Free Replicated Data Types (CRDTs) core primitives.
//!
//! Provides deterministic state-based CRDTs:
//! - [`LwwRegister`] — Last-Write-Wins Register with node ID tie-breaking.
//! - [`OrSet`] — Observed-Remove Set (Add-Wins Set) with unique element tag vectors.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use serde::{Deserialize, Serialize};

/// Last-Write-Wins Register (LWW-Register).
///
/// An immutable/replicated cell where the write with the highest timestamp wins.
/// Equal timestamps are deterministically broken using lexical comparison of `author_id`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LwwRegister<T> {
    /// Contained value.
    pub value: T,
    /// Lamport or Unix timestamp in milliseconds when written.
    pub timestamp_ms: u64,
    /// Unique identifier of the node that authored the write.
    pub author_id: String,
}

impl<T: Debug> Debug for LwwRegister<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LwwRegister")
            .field("value", &self.value)
            .field("timestamp_ms", &self.timestamp_ms)
            .field("author_id", &self.author_id)
            .finish()
    }
}

impl<T: Clone> LwwRegister<T> {
    /// Create a new register with the given value, timestamp, and author.
    pub fn new(value: T, timestamp_ms: u64, author_id: impl Into<String>) -> Self {
        Self {
            value,
            timestamp_ms,
            author_id: author_id.into(),
        }
    }

    /// Update value if `(timestamp_ms, author_id)` is strictly greater than current.
    pub fn update(&mut self, value: T, timestamp_ms: u64, author_id: &str) -> bool {
        if timestamp_ms > self.timestamp_ms
            || (timestamp_ms == self.timestamp_ms && author_id > self.author_id.as_str())
        {
            self.value = value;
            self.timestamp_ms = timestamp_ms;
            self.author_id = author_id.to_string();
            true
        } else {
            false
        }
    }

    /// Merge with another register, retaining the winning write.
    pub fn merge(&mut self, other: &Self) {
        if other.timestamp_ms > self.timestamp_ms
            || (other.timestamp_ms == self.timestamp_ms && other.author_id > self.author_id)
        {
            self.value = other.value.clone();
            self.timestamp_ms = other.timestamp_ms;
            self.author_id = other.author_id.clone();
        }
    }
}

/// An element tag in an Observed-Remove Set (OR-Set).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Tag {
    /// Timestamp when added.
    pub timestamp_ms: u64,
    /// Origin node ID.
    pub node_id: String,
    /// Monotonic sequence counter.
    pub seq: u64,
}

/// Observed-Remove Set (OR-Set / Add-Wins Set).
///
/// Elements can be added and removed concurrently. Removals only remove previously
/// observed additions. An addition concurrent with a removal wins.
#[derive(Clone, Serialize, Deserialize)]
pub struct OrSet<T: Ord + Clone> {
    /// Added elements mapped to their unique observation tags.
    pub adds: BTreeMap<T, BTreeSet<Tag>>,
    /// Tombstoned tags that have been removed.
    pub tombstones: BTreeSet<Tag>,
}

impl<T: Debug + Ord + Clone> Debug for OrSet<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrSet")
            .field("adds", &self.adds)
            .field("tombstones", &self.tombstones)
            .finish()
    }
}

impl<T: Ord + Clone> Default for OrSet<T> {
    fn default() -> Self {
        Self {
            adds: BTreeMap::new(),
            tombstones: BTreeSet::new(),
        }
    }
}

impl<T: Ord + Clone> OrSet<T> {
    /// Create a new empty OR-Set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an element to the set with a new observation tag.
    pub fn add(&mut self, item: T, tag: Tag) {
        self.adds.entry(item).or_default().insert(tag);
    }

    /// Remove an element from the set by moving all its current tags to tombstones.
    pub fn remove(&mut self, item: &T) -> bool {
        if let Some(tags) = self.adds.remove(item) {
            for tag in tags {
                self.tombstones.insert(tag);
            }
            true
        } else {
            false
        }
    }

    /// Check if the set contains an active (non-tombstoned) instance of the element.
    pub fn contains(&self, item: &T) -> bool {
        if let Some(tags) = self.adds.get(item) {
            tags.iter().any(|tag: &Tag| !self.tombstones.contains(tag))
        } else {
            false
        }
    }

    /// List active elements present in the set.
    pub fn elements(&self) -> Vec<T> {
        self.adds
            .iter()
            .filter(|(_, tags)| tags.iter().any(|tag| !self.tombstones.contains(tag)))
            .map(|(item, _)| item.clone())
            .collect()
    }

    /// Merge another OR-Set into this one.
    pub fn merge(&mut self, other: &Self) {
        // Union tombstones
        for tag in &other.tombstones {
            self.tombstones.insert(tag.clone());
        }

        // Union adds
        for (item, tags) in &other.adds {
            let entry = self.adds.entry(item.clone()).or_default();
            for tag in tags {
                entry.insert(tag.clone());
            }
        }

        // Clean up adds whose tags are entirely tombstones
        self.adds
            .retain(|_, tags| tags.iter().any(|tag| !self.tombstones.contains(tag)));
    }

    /// Count of active elements.
    pub fn len(&self) -> usize {
        self.elements().len()
    }

    /// Whether the active set is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lww_register_convergence() {
        let mut r1 = LwwRegister::new("camera_1".to_string(), 100, "node-A");
        let r2 = LwwRegister::new("camera_1_renamed".to_string(), 150, "node-B");

        r1.merge(&r2);
        assert_eq!(r1.value, "camera_1_renamed");
        assert_eq!(r1.timestamp_ms, 150);
        assert_eq!(r1.author_id, "node-B");

        // Stale write rejected
        let r_stale = LwwRegister::new("camera_1_old".to_string(), 50, "node-C");
        r1.merge(&r_stale);
        assert_eq!(r1.value, "camera_1_renamed");

        // Same timestamp broken by author_id
        let r_tie = LwwRegister::new("camera_1_tie".to_string(), 150, "node-Z");
        r1.merge(&r_tie);
        assert_eq!(r1.value, "camera_1_tie");
    }

    #[test]
    fn test_orset_add_remove_convergence() {
        let mut s1 = OrSet::new();
        let mut s2 = OrSet::new();

        let tag1 = Tag {
            timestamp_ms: 100,
            node_id: "node-1".into(),
            seq: 1,
        };
        let tag2 = Tag {
            timestamp_ms: 110,
            node_id: "node-2".into(),
            seq: 1,
        };

        s1.add("peer-A", tag1);
        s2.add("peer-B", tag2);

        // s1 removes peer-A
        assert!(s1.remove(&"peer-A"));
        assert!(!s1.contains(&"peer-A"));

        // Merge s1 and s2
        s1.merge(&s2);
        s2.merge(&s1);

        assert!(!s1.contains(&"peer-A"));
        assert!(!s2.contains(&"peer-A"));
        assert!(s1.contains(&"peer-B"));
        assert!(s2.contains(&"peer-B"));
        assert_eq!(s1.elements(), s2.elements());
    }
}
