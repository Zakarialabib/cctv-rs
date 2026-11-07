//! Integration tests for multi-stream composite recording profiles,
//! GOP-cached NALU muxing with timestamp normalization, and fleet reconciliation.

use std::future::Future;

use cctv_rs::{
    AudioSource, CameraConfig, CameraId, CompositeInputConfig, CompositeStreamMux, GridLayout,
    ProfileError, Reconciler, RecordingContainer, RecordingProfile, StreamInfo, StreamQuality,
    VideoCodec,
};

fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let waker = std::task::Waker::noop();
    let mut cx = std::task::Context::from_waker(waker);
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
        std::thread::yield_now();
    }
}

#[test]
fn test_recording_profile_validation_and_codecs() {
    let cam1 = CameraId::new("cam-front");
    let cam2 = CameraId::new("cam-rear");

    let profile_mp4 = RecordingProfile::new(
        vec![cam1.clone(), cam2.clone()],
        GridLayout::SplitHorizontal,
        Some(AudioSource::Camera(cam1.clone())),
        RecordingContainer::Mp4,
    );

    let stream_h264_main = StreamInfo {
        uri: "rtsp://camera1/main".to_string(),
        profile: "main".to_string(),
        quality: StreamQuality::Main,
        width: Some(1920),
        height: Some(1080),
        codec: Some("H264".to_string()),
    };

    let stream_h265_sub = StreamInfo {
        uri: "rtsp://camera2/sub".to_string(),
        profile: "sub".to_string(),
        quality: StreamQuality::Sub,
        width: Some(640),
        height: Some(360),
        codec: Some("H265".to_string()),
    };

    let stream_h264_sub = StreamInfo {
        uri: "rtsp://camera2/sub".to_string(),
        profile: "sub".to_string(),
        quality: StreamQuality::Sub,
        width: Some(640),
        height: Some(360),
        codec: Some("h264".to_string()),
    };

    // MP4 rejects mixed codecs (H264 + H265)
    let mixed_err = profile_mp4
        .validate_streams(&[stream_h264_main.clone(), stream_h265_sub.clone()])
        .unwrap_err();
    assert!(matches!(mixed_err, ProfileError::MixedCodecs { .. }));

    // MP4 accepts homogeneous codecs (H264 + H264)
    assert!(profile_mp4
        .validate_streams(&[stream_h264_main.clone(), stream_h264_sub.clone()])
        .is_ok());

    // Mkv allows mixed codecs
    let profile_mkv = RecordingProfile::new(
        vec![cam1.clone(), cam2.clone()],
        GridLayout::SplitHorizontal,
        None,
        RecordingContainer::Mkv,
    );
    assert!(profile_mkv
        .validate_streams(&[stream_h264_main, stream_h265_sub])
        .is_ok());
}

#[test]
fn test_composite_mux_timestamp_normalization() {
    block_on(async {
        let cam_a = CameraId::new("cam-a");
        let cam_b = CameraId::new("cam-b");

        let profile = RecordingProfile::new(
            vec![cam_a.clone(), cam_b.clone()],
            GridLayout::SplitVertical,
            None,
            RecordingContainer::Ts,
        );

        let inputs = vec![
            CompositeInputConfig::new(cam_a.clone(), 0, VideoCodec::H264),
            CompositeInputConfig::new(cam_b.clone(), 1, VideoCodec::H264),
        ];

        let mux = CompositeStreamMux::new(profile, inputs).expect("composite mux");
        let mut rx = mux.subscribe();

        // Stream 0 arrives with raw RTP 180_000 at common clock 5000 ms
        mux.ingest_access_unit(0, 180_000, 5000, vec![vec![0x67, 0x42, 0x00]])
            .await;
        let n0 = rx.recv().await.expect("nalu");
        assert_eq!(n0.stream_index, 0);
        assert_eq!(n0.normalized_pts_ms, 5000);

        // Stream 1 arrives with completely uncoordinated RTP timestamp 9_000_000 at common clock 5020 ms
        mux.ingest_access_unit(1, 9_000_000, 5020, vec![vec![0x67, 0x42, 0x00]])
            .await;
        let n1 = rx.recv().await.expect("nalu");
        assert_eq!(n1.stream_index, 1);
        assert_eq!(n1.normalized_pts_ms, 5020);

        // Stream 0 delivers next frame 900 ticks later (900 / 90 = 10 ms delta)
        mux.ingest_access_unit(0, 180_900, 5010, vec![vec![0x41, 0x9A]])
            .await;
        let n0_2 = rx.recv().await.expect("nalu");
        assert_eq!(n0_2.stream_index, 0);
        assert_eq!(n0_2.normalized_pts_ms, 5010);
    });
}

#[test]
fn test_fleet_reconciliation_applies_group_profile() {
    let mut reconciler = Reconciler::default();

    let profile = RecordingProfile::new(
        vec![CameraId::new("hall-1"), CameraId::new("hall-2")],
        GridLayout::Quad,
        None,
        RecordingContainer::Mp4,
    );

    reconciler.set_group_profile("hallway", profile.clone());

    let mut fleet = vec![
        CameraConfig::new("hall-1", "onvif", "10.0.1.10").with_group("hallway"),
        CameraConfig::new("hall-2", "onvif", "10.0.1.11").with_group("hallway"),
        CameraConfig::new("yard-1", "onvif", "10.0.2.10").with_group("yard"),
    ];

    let updated = reconciler.reconcile_group_profiles(&mut fleet);
    assert_eq!(updated, vec!["hall-1", "hall-2"]);
    assert_eq!(fleet[0].recording_profile, Some(profile.clone()));
    assert_eq!(fleet[1].recording_profile, Some(profile));
    assert_eq!(fleet[2].recording_profile, None);
}
