//! Comprehensive integration tests for `cctv-rag`.

use cctv_core::{AuditEntry, CameraId, MotionEvent};
use cctv_rag::{
    AdjacentToEdge, CameraKind, CameraNode, IncidentChunk, IncidentCoalescer, IncidentSeverity,
    KnowledgeBaseConfig, KnowledgeBaseIndex, PresetNode, PtzReachesEdge, SpatialGraph, ZoneKind,
    ZoneNode,
};

#[test]
fn test_coalescer_noise_suppression_motion_burst() {
    let mut coalescer = IncidentCoalescer::new(5_000, 50);
    let cam_id = CameraId::new("cam-dock");

    // Ingest 50 high frequency motion events over 2 seconds
    for i in 0..50 {
        let event = MotionEvent {
            camera_id: "cam-dock".to_string(),
            zone_id: "loading-bay".to_string(),
            ts: 1_000 + i * 40,
            confidence: 0.6 + (i as f32 * 0.005),
        };
        coalescer.ingest_motion(&cam_id, &event, 10_000 + i * 40);
    }

    // Still in same window
    assert_eq!(coalescer.open_window_count(), 1);
    let flushed_early = coalescer.flush_expired(11_000);
    assert!(flushed_early.is_empty());

    // Window expires at 15_001
    let flushed = coalescer.flush_expired(15_001);
    assert_eq!(flushed.len(), 1);

    let chunk = &flushed[0];
    assert_eq!(chunk.telemetry.motion_count, 50);
    assert_eq!(chunk.severity, IncidentSeverity::Warning);
    assert!(chunk.tags.contains(&"motion_burst".to_string()));
    assert!(chunk.narrative.contains("Recorded 50 motion detections"));
}

#[test]
fn test_spatial_handoff_filtering_and_reach() {
    let mut graph = SpatialGraph::new();

    // Setup Zones: gate and lobby (adjacent)
    graph.add_zone(ZoneNode {
        zone_id: "zone-gate".to_string(),
        label: "Outer Gate".to_string(),
        zone_kind: ZoneKind::Perimeter,
        polygon: vec![],
    });
    graph.add_zone(ZoneNode {
        zone_id: "zone-lobby".to_string(),
        label: "Main Lobby".to_string(),
        zone_kind: ZoneKind::Entry,
        polygon: vec![],
    });
    graph.add_adjacent(AdjacentToEdge {
        from_zone: "zone-gate".to_string(),
        to_zone: "zone-lobby".to_string(),
        distance_m: 15.0,
        handover_eligible: true,
    });

    // PTZ Camera 1: Healthy with available sub-stream budget
    let mut cam1 = CameraNode::new(CameraId::new("ptz-gate-1"), CameraKind::Ptz, 2, 4);
    cam1.active_sub_streams = 1;
    graph.add_camera(cam1);
    graph.add_preset(PresetNode {
        camera_id: CameraId::new("ptz-gate-1"),
        preset_token: "preset-p1".to_string(),
        label: "Gate Zoom".to_string(),
        pan_tilt_zoom: None,
    });
    graph.add_ptz_reaches(PtzReachesEdge {
        camera_id: CameraId::new("ptz-gate-1"),
        preset_token: "preset-p1".to_string(),
        zone_id: "zone-gate".to_string(),
        transit_estimate_ms: 600,
    });

    // PTZ Camera 2: Exhausted sub-stream budget
    let mut cam2 = CameraNode::new(CameraId::new("ptz-gate-busy"), CameraKind::Ptz, 2, 2);
    cam2.active_sub_streams = 2; // Full
    graph.add_camera(cam2);
    graph.add_preset(PresetNode {
        camera_id: CameraId::new("ptz-gate-busy"),
        preset_token: "preset-busy".to_string(),
        label: "Gate Zoom Fast".to_string(),
        pan_tilt_zoom: None,
    });
    graph.add_ptz_reaches(PtzReachesEdge {
        camera_id: CameraId::new("ptz-gate-busy"),
        preset_token: "preset-busy".to_string(),
        zone_id: "zone-gate".to_string(),
        transit_estimate_ms: 200,
    });

    // PTZ Camera 3: Reaches adjacent lobby
    let mut cam3 = CameraNode::new(CameraId::new("ptz-lobby"), CameraKind::Ptz, 1, 2);
    cam3.active_sub_streams = 0;
    graph.add_camera(cam3);
    graph.add_preset(PresetNode {
        camera_id: CameraId::new("ptz-lobby"),
        preset_token: "preset-lobby-entry".to_string(),
        label: "Lobby Entry Presets".to_string(),
        pan_tilt_zoom: None,
    });
    graph.add_ptz_reaches(PtzReachesEdge {
        camera_id: CameraId::new("ptz-lobby"),
        preset_token: "preset-lobby-entry".to_string(),
        zone_id: "zone-lobby".to_string(),
        transit_estimate_ms: 1000,
    });

    // Query handoff candidates for zone-gate
    let candidates = graph.find_handoff_candidates("zone-gate");

    // Expect:
    // - ptz-gate-busy is excluded (budget exhausted)
    // - ptz-gate-1 is direct coverage (transit 600ms)
    // - ptz-lobby is adjacent handover (transit 1000ms)
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].camera_id, CameraId::new("ptz-gate-1"));
    assert!(candidates[0].direct_coverage);
    assert_eq!(candidates[0].transit_estimate_ms, 600);

    assert_eq!(candidates[1].camera_id, CameraId::new("ptz-lobby"));
    assert!(!candidates[1].direct_coverage);
    assert_eq!(candidates[1].transit_estimate_ms, 1000);
}

#[test]
fn test_ringbuffer_memory_cap_enforcement() {
    let config = KnowledgeBaseConfig {
        max_incidents: 3,
        window_ms: 5_000,
        max_open_windows: 10,
    };
    let mut index = KnowledgeBaseIndex::with_config(config);

    for i in 1..=5 {
        index.insert_incident(IncidentChunk {
            chunk_id: format!("cam-test:{}-{}", i * 1000, (i + 1) * 1000),
            time_window: (i * 1000, (i + 1) * 1000),
            camera_id: CameraId::new(format!("cam-{}", i)),
            zone_id: None,
            severity: IncidentSeverity::Info,
            tags: vec!["test".to_string()],
            narrative: format!("Test incident {}", i),
            telemetry: Default::default(),
        });
    }

    // Only 3 incidents should remain in memory (chunks 3, 4, 5)
    let envelope = index.query("Test incident", 10).expect("query success");
    assert_eq!(envelope.chunks.len(), 3);

    let doc_ids: Vec<String> = envelope.chunks.into_iter().map(|c| c.id).collect();
    assert!(!doc_ids.contains(&"cam-test:1000-2000".to_string()));
    assert!(!doc_ids.contains(&"cam-test:2000-3000".to_string()));
    assert!(doc_ids.contains(&"cam-test:3000-4000".to_string()));
    assert!(doc_ids.contains(&"cam-test:4000-5000".to_string()));
    assert!(doc_ids.contains(&"cam-test:5000-6000".to_string()));
}

#[test]
fn test_diagnostic_playbook_retrieval_and_filtering() {
    let index = KnowledgeBaseIndex::new();

    // Query for 401 Digest nonce timeout
    let envelope = index
        .query("RTSP 401 Unauthorized nonce expired", 2)
        .expect("query success");
    assert!(!envelope.chunks.is_empty());
    assert!(envelope
        .formatted_prompt
        .contains("rtsp_digest_nonce_stale"));
    assert!(envelope
        .formatted_prompt
        .contains("unauthenticated DESCRIBE"));

    // Query for GOP starvation
    let envelope_gop = index
        .query("UDP packet drops and keyframe decode error", 2)
        .expect("query success");
    assert!(!envelope_gop.chunks.is_empty());
    assert!(envelope_gop
        .formatted_prompt
        .contains("rtp_udp_gop_starvation"));
    assert!(envelope_gop.formatted_prompt.contains("interleaved TCP"));
}

#[test]
fn test_transport_metrics_and_audit_coalescing() {
    let mut coalescer = IncidentCoalescer::new(10_000, 10);
    let cam = CameraId::new("cam-metrics");

    // Ingest transport metrics
    coalescer.ingest_transport_metrics(&cam, 15.0, 35, 2, 100_000);

    // Ingest audit entry
    let audit = AuditEntry {
        camera_id: "cam-metrics".to_string(),
        what: "ClockHealing applied offset 350s".to_string(),
        grade: 2,
        at_ms: 100_500,
    };
    coalescer.ingest_audit(&audit, 100_500);

    // Expire window
    let chunks = coalescer.flush_expired(110_001);
    assert_eq!(chunks.len(), 1);

    let chunk = &chunks[0];
    assert_eq!(chunk.telemetry.dropped_frames, 35);
    assert_eq!(chunk.telemetry.starved_gops, 2);
    assert!(chunk.tags.contains(&"gop_starvation".to_string()));
    assert!(chunk.tags.contains(&"frame_drops".to_string()));
    assert!(chunk.tags.contains(&"clock_skew".to_string()));
    assert_eq!(chunk.severity, IncidentSeverity::Degraded);
    assert!(chunk
        .narrative
        .contains("35 dropped frames, 2 starved GOPs"));
    assert!(chunk.narrative.contains("ClockHealing applied offset 350s"));
}
