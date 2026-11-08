//! Tests for driver registry resolution ladder, web UI analyser proposals, and Profile G types.

use cctv_core::ProbeResult;
use cctv_rs::{
    analyse_web_ui, priority, AuthScheme, CameraConfig, DriverRegistry, FingerprintRule,
    InMemoryRegistry, PlaybackUri, RecordingItem, RecordingSummary, RtspTemplate, TrackInformation,
};
use std::sync::Arc;

#[test]
fn test_facade_reexports_and_ladder_resolution() {
    let mut registry = InMemoryRegistry::new();

    let adapter = Arc::new(
        cctv_driver::mock::MockDriverAdapter::new("custom_vendor")
            .with_priority(priority::SPECIFIC)
            .with_templates(vec![RtspTemplate::new("/live/ch0", 554)]),
    );
    registry.register(adapter);

    let config = CameraConfig::new("cam1", "custom_vendor", "192.168.1.100");
    let resolved = registry.resolve(&config, None).expect("resolved adapter");
    assert_eq!(resolved.driver_key(), "custom_vendor");
    assert_eq!(resolved.priority(), priority::SPECIFIC);
    assert_eq!(resolved.rtsp_templates().len(), 1);

    let rule = FingerprintRule::for_driver("custom_vendor");
    let probe = ProbeResult {
        vendor: "custom_vendor".to_string(),
        model: "model_x".to_string(),
        firmware: None,
        ip: None,
        mac: None,
        ptz_supported: false,
        ai_supported: false,
        stream_url: None,
    };
    assert!(rule.matches(&probe));
}

#[test]
fn test_analyser_proposals_in_facade() {
    let snippet = r#"
        // Axis camera interface
        var liveFeed = "rtsp://10.0.0.50/axis-media/media.amp";
        var control = "/axis-cgi/param.cgi";
        var auth = "Authorization: Basic dXNlcjpwYXNz";
    "#;

    let proposal = analyse_web_ui(snippet);
    assert_eq!(proposal.vendor_hint, "Axis");
    assert_eq!(proposal.auth_scheme, AuthScheme::Basic);
    assert!(proposal
        .rtsp_paths
        .contains(&"/axis-media/media.amp".to_string()));
    let md = proposal.to_markdown_pr();
    assert!(md.contains("## Camera Driver Proposal"));
    assert!(md.contains("Axis"));
}

#[test]
fn test_profile_g_types_reexport() {
    let summary = RecordingSummary {
        total_recordings: 5,
        data_from: Some("2026-11-01T00:00:00Z".into()),
        data_until: Some("2026-11-09T00:00:00Z".into()),
    };
    assert_eq!(summary.total_recordings, 5);

    let item = RecordingItem {
        recording_token: "rec_1".into(),
        earliest_time: Some("2026-11-01T00:00:00Z".into()),
        latest_time: Some("2026-11-01T01:00:00Z".into()),
        tracks: vec![TrackInformation {
            track_token: "trk_1".into(),
            track_type: "Video".into(),
            description: None,
        }],
    };
    assert_eq!(item.recording_token, "rec_1");
    assert_eq!(item.tracks.len(), 1);

    let playback = PlaybackUri {
        uri: "rtsp://10.0.0.50:554/playback?rec=rec_1".into(),
        timeout: Some("PT30S".into()),
    };
    assert_eq!(playback.uri, "rtsp://10.0.0.50:554/playback?rec=rec_1");
}
