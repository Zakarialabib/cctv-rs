//! Replicated Device Registry synchronized using CRDTs.

use std::collections::HashMap;

use cctv_core::{CameraConfig, CameraType};
use serde::{Deserialize, Serialize};

use crate::crdt::{LwwRegister, OrSet, Tag};

/// Replicated camera configuration record with CRDT conflict resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCameraRecord {
    /// Unique camera ID.
    pub id: String,
    /// Driver identifier (LWW).
    pub driver: LwwRegister<String>,
    /// Host name or IP (LWW).
    pub host: LwwRegister<String>,
    /// HTTP/control port (LWW).
    pub port: LwwRegister<Option<u16>>,
    /// RTSP port (LWW).
    pub rtsp_port: LwwRegister<Option<u16>>,
    /// HTTPS enabled (LWW).
    pub https: LwwRegister<bool>,
    /// Service URL (LWW).
    pub service_url: LwwRegister<Option<String>>,
    /// Friendly label (LWW).
    pub label: LwwRegister<Option<String>>,
    /// Camera hardware type (LWW).
    pub ty: LwwRegister<CameraType>,
}

/// A replicated device registry synchronizing cameras across edge nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDeviceRegistry {
    /// Local node ID.
    pub node_id: String,
    /// Sequence counter.
    seq: u64,
    /// Set of active cameras (OR-Set).
    camera_set: OrSet<String>,
    /// Camera configuration registers.
    cameras: HashMap<String, SyncCameraRecord>,
}

impl SyncDeviceRegistry {
    /// Create a new device registry for `node_id`.
    pub fn new(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            seq: 0,
            camera_set: OrSet::new(),
            cameras: HashMap::new(),
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

    /// Register or update a camera configuration.
    pub fn register_camera(&mut self, config: CameraConfig, now_ms: u64) {
        let id = config.id.clone();
        let tag = self.next_tag(now_ms);

        self.camera_set.add(id.clone(), tag);

        let record = self
            .cameras
            .entry(id.clone())
            .or_insert_with(|| SyncCameraRecord {
                id: id.clone(),
                driver: LwwRegister::new(config.driver.clone(), now_ms, &self.node_id),
                host: LwwRegister::new(config.host.clone(), now_ms, &self.node_id),
                port: LwwRegister::new(config.port, now_ms, &self.node_id),
                rtsp_port: LwwRegister::new(config.rtsp_port, now_ms, &self.node_id),
                https: LwwRegister::new(config.https, now_ms, &self.node_id),
                service_url: LwwRegister::new(config.service_url.clone(), now_ms, &self.node_id),
                label: LwwRegister::new(config.label.clone(), now_ms, &self.node_id),
                ty: LwwRegister::new(config.ty, now_ms, &self.node_id),
            });

        record.driver.update(config.driver, now_ms, &self.node_id);
        record.host.update(config.host, now_ms, &self.node_id);
        record.port.update(config.port, now_ms, &self.node_id);
        record
            .rtsp_port
            .update(config.rtsp_port, now_ms, &self.node_id);
        record.https.update(config.https, now_ms, &self.node_id);
        record
            .service_url
            .update(config.service_url, now_ms, &self.node_id);
        record.label.update(config.label, now_ms, &self.node_id);
        record.ty.update(config.ty, now_ms, &self.node_id);
    }

    /// Unregister/remove a camera from the active registry.
    pub fn unregister_camera(&mut self, id: &str) -> bool {
        self.camera_set.remove(&id.to_string())
    }

    /// Check if a camera is registered.
    pub fn contains(&self, id: &str) -> bool {
        self.camera_set.contains(&id.to_string())
    }

    /// Retrieve camera configuration by ID.
    pub fn get(&self, id: &str) -> Option<CameraConfig> {
        if !self.contains(id) {
            return None;
        }
        self.cameras.get(id).map(|rec| {
            let mut cfg = CameraConfig::new(
                rec.id.clone(),
                rec.driver.value.clone(),
                rec.host.value.clone(),
            );
            cfg.port = rec.port.value;
            cfg.rtsp_port = rec.rtsp_port.value;
            cfg.https = rec.https.value;
            cfg.service_url = rec.service_url.value.clone();
            cfg.label = rec.label.value.clone();
            cfg.ty = rec.ty.value;
            cfg
        })
    }

    /// List all registered cameras.
    pub fn list_cameras(&self) -> Vec<CameraConfig> {
        self.camera_set
            .elements()
            .into_iter()
            .filter_map(|id| self.get(&id))
            .collect()
    }

    /// Merge state from a remote `SyncDeviceRegistry`.
    pub fn merge(&mut self, other: &Self) {
        self.camera_set.merge(&other.camera_set);

        for (id, other_rec) in &other.cameras {
            match self.cameras.get_mut(id) {
                Some(local_rec) => {
                    local_rec.driver.merge(&other_rec.driver);
                    local_rec.host.merge(&other_rec.host);
                    local_rec.port.merge(&other_rec.port);
                    local_rec.rtsp_port.merge(&other_rec.rtsp_port);
                    local_rec.https.merge(&other_rec.https);
                    local_rec.service_url.merge(&other_rec.service_url);
                    local_rec.label.merge(&other_rec.label);
                    local_rec.ty.merge(&other_rec.ty);
                }
                None => {
                    self.cameras.insert(id.clone(), other_rec.clone());
                }
            }
        }
    }
}
