//! Spatial topology graph and autonomous camera PTZ handoff engine.
//!
//! Models physical camera placement, zones, PTZ presets, and adjacent relationships
//! to enable cross-camera handoff tracking and structured knowledge chunk extraction.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use cctv_core::CameraId;

/// Classification of physical camera hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CameraKind {
    /// Fixed focal length / stationary mount.
    Fixed,
    /// Motorized Pan-Tilt-Zoom camera.
    Ptz,
    /// Multi-sensor 180° / 360° panoramic camera.
    Panoramic,
    /// USB Video Class / PTP local bridge camera.
    Uvc,
}

/// A camera node in the physical space graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraNode {
    /// Camera identifier.
    pub camera_id: CameraId,
    /// Hardware type.
    pub kind: CameraKind,
    /// Maximum concurrent main-stream sessions supported by hardware.
    pub max_main_streams: u8,
    /// Maximum concurrent sub-stream sessions supported by hardware.
    pub max_sub_streams: u8,
    /// Currently active main-stream sessions.
    pub active_main_streams: u8,
    /// Currently active sub-stream sessions.
    pub active_sub_streams: u8,
    /// Operational reachability and health status.
    pub healthy: bool,
}

impl CameraNode {
    /// Create a new camera node with the given hardware constraints.
    pub fn new(camera_id: CameraId, kind: CameraKind, max_main: u8, max_sub: u8) -> Self {
        Self {
            camera_id,
            kind,
            max_main_streams: max_main,
            max_sub_streams: max_sub,
            active_main_streams: 0,
            active_sub_streams: 0,
            healthy: true,
        }
    }

    /// Whether this camera has capacity for an additional sub-stream.
    pub fn has_sub_stream_budget(&self) -> bool {
        self.active_sub_streams < self.max_sub_streams
    }

    /// Whether this camera has capacity for an additional main-stream.
    pub fn has_main_stream_budget(&self) -> bool {
        self.active_main_streams < self.max_main_streams
    }
}

/// Semantic category of a monitored zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneKind {
    /// Building or facility ingress / egress.
    Entry,
    /// Freight, shipping, or loading dock.
    LoadingDock,
    /// Point of sale / cash register area.
    Cashier,
    /// Outer boundary or fence line.
    Perimeter,
    /// Custom application-specific zone.
    Custom(String),
}

/// A monitored physical zone node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoneNode {
    /// Unique identifier for the zone.
    pub zone_id: String,
    /// Human-readable label (e.g., "North Gate Loading Dock").
    pub label: String,
    /// Semantic classification.
    pub zone_kind: ZoneKind,
    /// Optional polygon vertices in normalized frame coordinates `[(x, y), ...]`.
    pub polygon: Vec<(f32, f32)>,
}

/// A stored PTZ preset position on a camera.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresetNode {
    /// The camera that owns this preset.
    pub camera_id: CameraId,
    /// The device preset token/identifier.
    pub preset_token: String,
    /// Human-readable preset name (e.g., "Preset 1: Cash Register").
    pub label: String,
    /// Normalized pan, tilt, zoom vector if known `(pan, tilt, zoom)`.
    pub pan_tilt_zoom: Option<(f32, f32, f32)>,
}

/// Edge: Camera covers a physical zone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoversEdge {
    /// Camera ID.
    pub camera_id: CameraId,
    /// Zone ID.
    pub zone_id: String,
    /// Field of view angle coverage in degrees.
    pub fov_deg: f32,
    /// Whether this is the primary camera assigned to monitor the zone.
    pub primary: bool,
}

/// Edge: Physical adjacency between two zones.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdjacentToEdge {
    /// Origin zone ID.
    pub from_zone: String,
    /// Adjacent destination zone ID.
    pub to_zone: String,
    /// Physical distance in meters between zones.
    pub distance_m: f32,
    /// Whether moving subjects frequently transition between these zones.
    pub handover_eligible: bool,
}

/// Edge: A camera's PTZ preset targets a specific zone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PtzReachesEdge {
    /// PTZ Camera ID.
    pub camera_id: CameraId,
    /// Stored preset token.
    pub preset_token: String,
    /// Target zone ID.
    pub zone_id: String,
    /// Estimated movement transit time in milliseconds.
    pub transit_estimate_ms: u32,
}

/// A candidate camera for autonomous PTZ handover.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandoffCandidate {
    /// Camera identifier.
    pub camera_id: CameraId,
    /// Preset token to command.
    pub preset_token: String,
    /// Preset label if known.
    pub preset_label: String,
    /// Estimated time to move to the preset in ms.
    pub transit_estimate_ms: u32,
    /// Whether the target is directly in the triggered zone or an adjacent zone.
    pub direct_coverage: bool,
}

/// A structured knowledge chunk serialized for LLM and RAG retrieval.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnowledgeChunk {
    /// Unique chunk identifier.
    pub chunk_id: String,
    /// Subject camera or entity.
    pub entity_id: String,
    /// Natural language representation of spatial and capability knowledge.
    pub narrative: String,
    /// Associated entity tags.
    pub tags: Vec<String>,
}

/// Spatial and camera capability topology graph.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpatialGraph {
    /// Camera nodes keyed by `CameraId`.
    pub cameras: BTreeMap<CameraId, CameraNode>,
    /// Zone nodes keyed by zone ID.
    pub zones: BTreeMap<String, ZoneNode>,
    /// Preset nodes keyed by `(CameraId, preset_token)`.
    pub presets: BTreeMap<(CameraId, String), PresetNode>,
    /// Camera-to-Zone coverage edges.
    pub covers: Vec<CoversEdge>,
    /// Zone-to-Zone adjacency edges.
    pub adjacencies: Vec<AdjacentToEdge>,
    /// PTZ-preset-to-Zone reaches edges.
    pub ptz_reaches: Vec<PtzReachesEdge>,
}

impl SpatialGraph {
    /// Create an empty spatial graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or update a camera node.
    pub fn add_camera(&mut self, camera: CameraNode) {
        self.cameras.insert(camera.camera_id.clone(), camera);
    }

    /// Add or update a zone node.
    pub fn add_zone(&mut self, zone: ZoneNode) {
        self.zones.insert(zone.zone_id.clone(), zone);
    }

    /// Add or update a PTZ preset node.
    pub fn add_preset(&mut self, preset: PresetNode) {
        self.presets.insert(
            (preset.camera_id.clone(), preset.preset_token.clone()),
            preset,
        );
    }

    /// Add a coverage edge.
    pub fn add_covers(&mut self, edge: CoversEdge) {
        self.covers.push(edge);
    }

    /// Add an adjacency edge.
    pub fn add_adjacent(&mut self, edge: AdjacentToEdge) {
        self.adjacencies.push(edge);
    }

    /// Add a PTZ reaches edge.
    pub fn add_ptz_reaches(&mut self, edge: PtzReachesEdge) {
        self.ptz_reaches.push(edge);
    }

    /// Query for healthy PTZ cameras that can reach `zone_id` or adjacent handover zones,
    /// constrained by available sub-stream budget, sorted ascending by transit estimate.
    pub fn find_handoff_candidates(&self, zone_id: &str) -> Vec<HandoffCandidate> {
        let mut candidates = Vec::new();
        let mut seen = BTreeSet::new();

        // 1. Direct coverage via PTZ preset to `zone_id`
        for reach in &self.ptz_reaches {
            if reach.zone_id == zone_id {
                if let Some(cam) = self.cameras.get(&reach.camera_id) {
                    if cam.healthy && cam.kind == CameraKind::Ptz && cam.has_sub_stream_budget() {
                        let key = (reach.camera_id.clone(), reach.preset_token.clone());
                        if seen.insert(key) {
                            let label = self
                                .presets
                                .get(&(reach.camera_id.clone(), reach.preset_token.clone()))
                                .map(|p| p.label.clone())
                                .unwrap_or_else(|| reach.preset_token.clone());

                            candidates.push(HandoffCandidate {
                                camera_id: reach.camera_id.clone(),
                                preset_token: reach.preset_token.clone(),
                                preset_label: label,
                                transit_estimate_ms: reach.transit_estimate_ms,
                                direct_coverage: true,
                            });
                        }
                    }
                }
            }
        }

        // 2. Handover coverage via adjacent zones
        let mut adjacent_zones = Vec::new();
        for adj in &self.adjacencies {
            if adj.from_zone == zone_id && adj.handover_eligible {
                adjacent_zones.push((&adj.to_zone, adj.distance_m));
            } else if adj.to_zone == zone_id && adj.handover_eligible {
                adjacent_zones.push((&adj.from_zone, adj.distance_m));
            }
        }

        for (adj_zone, _dist) in adjacent_zones {
            for reach in &self.ptz_reaches {
                if reach.zone_id == *adj_zone {
                    if let Some(cam) = self.cameras.get(&reach.camera_id) {
                        if cam.healthy && cam.kind == CameraKind::Ptz && cam.has_sub_stream_budget()
                        {
                            let key = (reach.camera_id.clone(), reach.preset_token.clone());
                            if seen.insert(key) {
                                let label = self
                                    .presets
                                    .get(&(reach.camera_id.clone(), reach.preset_token.clone()))
                                    .map(|p| p.label.clone())
                                    .unwrap_or_else(|| reach.preset_token.clone());

                                candidates.push(HandoffCandidate {
                                    camera_id: reach.camera_id.clone(),
                                    preset_token: reach.preset_token.clone(),
                                    preset_label: label,
                                    transit_estimate_ms: reach.transit_estimate_ms,
                                    direct_coverage: false,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Sort ascending by transit time
        candidates.sort_by_key(|c| (c.transit_estimate_ms, !c.direct_coverage));
        candidates
    }

    /// Compile the spatial topology into structured, human/LLM-readable knowledge chunks.
    pub fn to_knowledge_chunks(&self) -> Vec<KnowledgeChunk> {
        let mut chunks = Vec::with_capacity(self.cameras.len());

        for (cam_id, cam) in &self.cameras {
            let mut parts = Vec::new();
            let kind_str = match cam.kind {
                CameraKind::Fixed => "Fixed camera",
                CameraKind::Ptz => "Motorized PTZ camera",
                CameraKind::Panoramic => "Panoramic camera",
                CameraKind::Uvc => "UVC/PTP camera",
            };
            parts.push(format!(
                "Camera {} is a {kind_str} (status: {}).",
                cam_id,
                if cam.healthy { "healthy" } else { "unhealthy" }
            ));

            parts.push(format!(
                "Stream budget: {}/{} main streams, {}/{} sub streams active.",
                cam.active_main_streams,
                cam.max_main_streams,
                cam.active_sub_streams,
                cam.max_sub_streams
            ));

            // Coverage
            let covers: Vec<_> = self
                .covers
                .iter()
                .filter(|c| c.camera_id == *cam_id)
                .map(|c| {
                    let label = self
                        .zones
                        .get(&c.zone_id)
                        .map(|z| z.label.as_str())
                        .unwrap_or(c.zone_id.as_str());
                    format!("{label} ({:.0}° FOV)", c.fov_deg)
                })
                .collect();

            if !covers.is_empty() {
                parts.push(format!("Covers zones: {}.", covers.join(", ")));
            }

            // Presets & reach
            if cam.kind == CameraKind::Ptz {
                let reaches: Vec<_> = self
                    .ptz_reaches
                    .iter()
                    .filter(|r| r.camera_id == *cam_id)
                    .map(|r| {
                        let preset_label = self
                            .presets
                            .get(&(cam_id.clone(), r.preset_token.clone()))
                            .map(|p| p.label.as_str())
                            .unwrap_or(r.preset_token.as_str());
                        format!(
                            "Preset '{}' reaches Zone '{}' (~{}ms)",
                            preset_label, r.zone_id, r.transit_estimate_ms
                        )
                    })
                    .collect();

                if !reaches.is_empty() {
                    parts.push(format!("PTZ reach presets: {}.", reaches.join("; ")));
                }
            }

            let narrative = parts.join(" ");
            let mut tags = vec!["topology".to_string(), format!("camera:{}", cam_id)];
            if cam.kind == CameraKind::Ptz {
                tags.push("ptz".to_string());
            }

            chunks.push(KnowledgeChunk {
                chunk_id: format!("topology:{}", cam_id),
                entity_id: cam_id.to_string(),
                narrative,
                tags,
            });
        }

        chunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ptz_handoff_candidate_search() {
        let mut graph = SpatialGraph::new();

        // Cam 1: Fixed covering loading-dock
        let cam_fixed = CameraNode::new(CameraId::new("cam-fixed"), CameraKind::Fixed, 2, 4);
        graph.add_camera(cam_fixed);

        // Cam 2: PTZ covering courtyard and reaches loading-dock preset
        let mut cam_ptz = CameraNode::new(CameraId::new("cam-ptz"), CameraKind::Ptz, 1, 2);
        cam_ptz.active_sub_streams = 0; // budget available
        graph.add_camera(cam_ptz);

        // Zone 1: loading-dock
        graph.add_zone(ZoneNode {
            zone_id: "loading-dock".to_string(),
            label: "Loading Dock Ramp".to_string(),
            zone_kind: ZoneKind::LoadingDock,
            polygon: vec![],
        });

        // Preset 1 on cam-ptz
        graph.add_preset(PresetNode {
            camera_id: CameraId::new("cam-ptz"),
            preset_token: "preset-dock".to_string(),
            label: "Loading Dock Preset".to_string(),
            pan_tilt_zoom: Some((0.5, 0.2, 1.0)),
        });

        // Edge: cam-ptz reaches loading-dock in 1200ms
        graph.add_ptz_reaches(PtzReachesEdge {
            camera_id: CameraId::new("cam-ptz"),
            preset_token: "preset-dock".to_string(),
            zone_id: "loading-dock".to_string(),
            transit_estimate_ms: 1200,
        });

        // Query handoff
        let candidates = graph.find_handoff_candidates("loading-dock");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].camera_id, CameraId::new("cam-ptz"));
        assert_eq!(candidates[0].preset_token, "preset-dock");
        assert_eq!(candidates[0].transit_estimate_ms, 1200);
        assert!(candidates[0].direct_coverage);
    }

    #[test]
    fn test_budget_exhaustion_disqualifies_handoff() {
        let mut graph = SpatialGraph::new();

        let mut cam_ptz = CameraNode::new(CameraId::new("cam-ptz"), CameraKind::Ptz, 1, 2);
        cam_ptz.active_sub_streams = 2; // budget FULL
        graph.add_camera(cam_ptz);

        graph.add_ptz_reaches(PtzReachesEdge {
            camera_id: CameraId::new("cam-ptz"),
            preset_token: "p1".to_string(),
            zone_id: "z1".to_string(),
            transit_estimate_ms: 500,
        });

        let candidates = graph.find_handoff_candidates("z1");
        assert!(
            candidates.is_empty(),
            "Full budget should disqualify camera"
        );
    }

    #[test]
    fn test_knowledge_chunk_compilation() {
        let mut graph = SpatialGraph::new();
        let cam = CameraNode::new(CameraId::new("cam-1"), CameraKind::Fixed, 2, 4);
        graph.add_camera(cam);

        let chunks = graph.to_knowledge_chunks();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0]
            .narrative
            .contains("Camera cam-1 is a Fixed camera"));
        assert!(chunks[0].narrative.contains("Stream budget"));
    }
}
