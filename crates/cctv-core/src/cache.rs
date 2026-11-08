//! Edge storage caching primitives.
//!
//! A runtime-free, alloc-only edge cache for the most recent frames and clips
//! keyed by camera. Implemented as a bounded ring-buffer with deterministic
//! eviction (oldest-first) and explicit capacity limits.

use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::string::String;

use crate::{MediaFile, Snapshot};

/// A bounded cache for recent snapshots and clips.
#[derive(Debug, Clone)]
pub struct EdgeCache {
    /// Maximum number of items to keep per camera.
    capacity_per_camera: usize,
    /// Ring-buffer of snapshots keyed by camera ID.
    snapshots: BTreeMap<String, VecDeque<Snapshot>>,
    /// Ring-buffer of clips keyed by camera ID.
    clips: BTreeMap<String, VecDeque<MediaFile>>,
}

impl EdgeCache {
    /// Creates a new `EdgeCache` with the given capacity per camera.
    ///
    /// If `capacity_per_camera` is 0, the cache will not store anything.
    pub fn new(capacity_per_camera: usize) -> Self {
        Self {
            capacity_per_camera,
            snapshots: BTreeMap::new(),
            clips: BTreeMap::new(),
        }
    }

    /// Pushes a new snapshot into the cache for the given camera.
    ///
    /// If the camera's snapshot buffer is at capacity, the oldest snapshot
    /// is deterministically evicted and returned.
    pub fn push_snapshot(&mut self, camera_id: &str, snapshot: Snapshot) -> Option<Snapshot> {
        if self.capacity_per_camera == 0 {
            return Some(snapshot);
        }

        let deque = self.snapshots.entry(camera_id.into()).or_default();

        let evicted = if deque.len() >= self.capacity_per_camera {
            deque.pop_front()
        } else {
            None
        };

        deque.push_back(snapshot);
        evicted
    }

    /// Pushes a new clip into the cache for the given camera.
    ///
    /// If the camera's clip buffer is at capacity, the oldest clip
    /// is deterministically evicted and returned.
    pub fn push_clip(&mut self, camera_id: &str, clip: MediaFile) -> Option<MediaFile> {
        if self.capacity_per_camera == 0 {
            return Some(clip);
        }

        let deque = self.clips.entry(camera_id.into()).or_default();

        let evicted = if deque.len() >= self.capacity_per_camera {
            deque.pop_front()
        } else {
            None
        };

        deque.push_back(clip);
        evicted
    }

    /// Gets an iterator over the snapshots for the given camera, from oldest to newest.
    pub fn snapshots(&self, camera_id: &str) -> impl Iterator<Item = &Snapshot> {
        self.snapshots
            .get(camera_id)
            .into_iter()
            .flat_map(|d| d.iter())
    }

    /// Gets an iterator over the clips for the given camera, from oldest to newest.
    pub fn clips(&self, camera_id: &str) -> impl Iterator<Item = &MediaFile> {
        self.clips.get(camera_id).into_iter().flat_map(|d| d.iter())
    }

    /// Clears all stored items for the given camera.
    pub fn clear_camera(&mut self, camera_id: &str) {
        self.snapshots.remove(camera_id);
        self.clips.remove(camera_id);
    }

    /// Clears the entire cache.
    pub fn clear(&mut self) {
        self.snapshots.clear();
        self.clips.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MediaFile, Snapshot};
    use alloc::string::ToString;

    fn mock_snapshot(content: &str) -> Snapshot {
        Snapshot {
            bytes: content.as_bytes().to_vec(),
            content_type: "image/jpeg".to_string(),
            width: None,
            height: None,
        }
    }

    fn mock_clip(id: &str) -> MediaFile {
        MediaFile {
            id: id.to_string(),
            filename: id.to_string(),
            bytes: alloc::vec![],
            mime_type: "video/mp4".to_string(),
            created_at: 0,
        }
    }

    #[test]
    fn test_capacity_and_eviction() {
        let mut cache = EdgeCache::new(2);

        let evict1 = cache.push_snapshot("cam1", mock_snapshot("1"));
        assert!(evict1.is_none());

        let evict2 = cache.push_snapshot("cam1", mock_snapshot("2"));
        assert!(evict2.is_none());

        let evict3 = cache.push_snapshot("cam1", mock_snapshot("3"));
        assert!(evict3.is_some());
        assert_eq!(evict3.unwrap().bytes, b"1");

        let snaps: Vec<_> = cache.snapshots("cam1").collect();
        assert_eq!(snaps.len(), 2);
        assert_eq!(snaps[0].bytes, b"2");
        assert_eq!(snaps[1].bytes, b"3");
    }

    #[test]
    fn test_zero_capacity() {
        let mut cache = EdgeCache::new(0);

        let evict = cache.push_snapshot("cam1", mock_snapshot("1"));
        assert!(evict.is_some());
        assert_eq!(evict.unwrap().bytes, b"1");

        assert_eq!(cache.snapshots("cam1").count(), 0);
    }

    #[test]
    fn test_clip_eviction() {
        let mut cache = EdgeCache::new(2);

        cache.push_clip("cam1", mock_clip("clip1"));
        cache.push_clip("cam1", mock_clip("clip2"));

        let evicted = cache.push_clip("cam1", mock_clip("clip3"));
        assert!(evicted.is_some());
        assert_eq!(evicted.unwrap().id, "clip1");

        let clips: Vec<_> = cache.clips("cam1").collect();
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].id, "clip2");
        assert_eq!(clips[1].id, "clip3");
    }

    #[test]
    fn test_multiple_cameras() {
        let mut cache = EdgeCache::new(2);

        cache.push_snapshot("cam1", mock_snapshot("c1_1"));
        cache.push_snapshot("cam2", mock_snapshot("c2_1"));
        cache.push_snapshot("cam1", mock_snapshot("c1_2"));
        cache.push_snapshot("cam2", mock_snapshot("c2_2"));
        cache.push_snapshot("cam1", mock_snapshot("c1_3")); // evicts c1_1

        let snaps1: Vec<_> = cache.snapshots("cam1").collect();
        assert_eq!(snaps1.len(), 2);
        assert_eq!(snaps1[0].bytes, b"c1_2");

        let snaps2: Vec<_> = cache.snapshots("cam2").collect();
        assert_eq!(snaps2.len(), 2);
        assert_eq!(snaps2[0].bytes, b"c2_1");
    }
}
