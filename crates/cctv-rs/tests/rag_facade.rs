//! Tests for facade RAG and spatial graph integration.

use cctv_rs::{CameraId, CameraKind, CameraNode, Facade, PresetNode, PtzReachesEdge, ZoneConfig};

#[test]
fn test_facade_rag_query_and_handoff() {
    let mut facade = Facade::new();

    // Register a zone
    facade.zone_save(
        "loading-dock",
        ZoneConfig::default_for(cctv_rs::Zone::point(0.0, 0.0, 1.0), 0.5),
    );

    // Configure spatial graph on facade
    let cam_ptz = CameraNode::new(CameraId::new("cam-ptz"), CameraKind::Ptz, 1, 2);
    facade.spatial_graph_mut().add_camera(cam_ptz);
    facade.spatial_graph_mut().add_preset(PresetNode {
        camera_id: CameraId::new("cam-ptz"),
        preset_token: "preset-dock".to_string(),
        label: "Loading Dock Preset".to_string(),
        pan_tilt_zoom: Some((0.5, 0.2, 1.0)),
    });
    facade.spatial_graph_mut().add_ptz_reaches(PtzReachesEdge {
        camera_id: CameraId::new("cam-ptz"),
        preset_token: "preset-dock".to_string(),
        zone_id: "loading-dock".to_string(),
        transit_estimate_ms: 800,
    });

    // Query handoff candidates through facade
    let candidates = facade.find_handoff_candidates("loading-dock");
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].camera_id, CameraId::new("cam-ptz"));
    assert_eq!(candidates[0].preset_token, "preset-dock");

    // Query RAG context through facade
    let envelope = facade
        .rag_query("Which camera covers loading-dock?", 3)
        .expect("query success");
    assert!(!envelope.chunks.is_empty());
    assert!(envelope.formatted_prompt.contains("cam-ptz"));
}
