//! 3-driver integration test: ONVIF mock + UVC mock + PTP mock
//!
//! Validates:
//! 1. State change -> event delivered in <= 200 ms.
//! 2. Backpressure drops oldest, not newest.
//! 3. Push-based reconciliation updates reconciler state and audits.

use std::sync::Arc;
use std::time::{Duration, Instant};

use cctv_core::{CameraConfig, CameraDesired, CameraState, CameraStateCurrent, CameraType};
use cctv_onvif::{Credentials, OnvifCameraBackend, OnvifSession};
use cctv_rs::{Facade, PtpAdapter, PtpStatus, UvcAdapter};
use cctv_stream_uvc::OsPlatform;
use cctv_transport::http::HttpConfig;

#[tokio::test]
async fn test_three_driver_observation_state_change_under_200ms() {
    let mut facade = Facade::new();

    // 1. Setup Mock ONVIF Backend
    let session = OnvifSession::new(
        "http://127.0.0.1:8080/onvif/device_service",
        Credentials {
            username: "admin".into(),
            password: "password".into(),
        },
        HttpConfig::default(),
    )
    .unwrap();
    let onvif_backend = Arc::new(OnvifCameraBackend::new(session));
    let onvif_cfg = CameraConfig::new("cam-onvif", "onvif", "127.0.0.1:8080");
    let onvif_handle = facade.register(onvif_cfg, onvif_backend.clone());

    // 2. Setup Mock UVC Backend
    let uvc_backend = Arc::new(UvcAdapter::new(
        "cam-uvc",
        "/dev/video0",
        OsPlatform::LinuxV4l2,
    ));
    let uvc_cfg = CameraConfig::new("cam-uvc", "uvc", "/dev/video0");
    let uvc_handle = facade.register(uvc_cfg, uvc_backend.clone());

    // 3. Setup Mock PTP Backend
    let ptp_backend = Arc::new(PtpAdapter::new("cam-ptp", "SN98765"));
    let ptp_cfg = CameraConfig::new("cam-ptp", "ptp", "SN98765");
    let ptp_handle = facade.register(ptp_cfg, ptp_backend.clone());

    // Subscribe to observation streams through Facade::observe()
    let mut onvif_stream = facade
        .observe(onvif_handle.id())
        .await
        .expect("onvif observe stream");
    let mut uvc_stream = facade
        .observe(uvc_handle.id())
        .await
        .expect("uvc observe stream");
    let mut ptp_stream = facade
        .observe(ptp_handle.id())
        .await
        .expect("ptp observe stream");

    // Test UVC: State change -> event <= 200 ms
    let start_uvc = Instant::now();
    uvc_backend.set_busy(true);
    let uvc_obs = tokio::time::timeout(Duration::from_millis(200), uvc_stream.next())
        .await
        .expect("UVC observation should arrive within 200ms")
        .expect("stream item")
        .expect("valid observation");
    let elapsed_uvc = start_uvc.elapsed();
    assert!(
        elapsed_uvc <= Duration::from_millis(200),
        "UVC delivery took {elapsed_uvc:?}"
    );
    assert_eq!(uvc_obs.camera_id, "cam-uvc");
    assert_eq!(uvc_obs.state, CameraStateCurrent::Away);
    assert!(!uvc_obs.online);

    // Test PTP: State change -> event <= 200 ms
    let start_ptp = Instant::now();
    ptp_backend.set_status(PtpStatus::StoreNotReady);
    let ptp_obs = tokio::time::timeout(Duration::from_millis(200), ptp_stream.next())
        .await
        .expect("PTP observation should arrive within 200ms")
        .expect("stream item")
        .expect("valid observation");
    let elapsed_ptp = start_ptp.elapsed();
    assert!(
        elapsed_ptp <= Duration::from_millis(200),
        "PTP delivery took {elapsed_ptp:?}"
    );
    assert_eq!(ptp_obs.camera_id, "cam-ptp");
    assert_eq!(ptp_obs.state, CameraStateCurrent::On);
    assert!(ptp_obs.online);

    // Test ONVIF: State change -> event <= 200 ms
    let start_onvif = Instant::now();
    onvif_backend.emit_observation(CameraStateCurrent::Standby, true, 1.0);
    let onvif_obs = tokio::time::timeout(Duration::from_millis(200), onvif_stream.next())
        .await
        .expect("ONVIF observation should arrive within 200ms")
        .expect("stream item")
        .expect("valid observation");
    let elapsed_onvif = start_onvif.elapsed();
    assert!(
        elapsed_onvif <= Duration::from_millis(200),
        "ONVIF delivery took {elapsed_onvif:?}"
    );
    assert_eq!(onvif_obs.camera_id, "127.0.0.1");
    assert_eq!(onvif_obs.state, CameraStateCurrent::Standby);
    assert!(onvif_obs.online);
}

#[tokio::test]
async fn test_observation_backpressure_drops_oldest_not_newest() {
    let mut facade = Facade::new();
    let uvc_backend = Arc::new(UvcAdapter::new(
        "cam-uvc-bp",
        "/dev/video1",
        OsPlatform::LinuxV4l2,
    ));
    let uvc_cfg = CameraConfig::new("cam-uvc-bp", "uvc", "/dev/video1");
    let handle = facade.register(uvc_cfg, uvc_backend.clone());

    // Subscribe to stream (channel buffer capacity is 128)
    let mut stream = facade.observe(handle.id()).await.expect("observe stream");

    // Emit 200 events without reading
    for _ in 1..=200 {
        uvc_backend.emit_observation(CameraStateCurrent::Streaming, true, 1.0);
    }

    // Read the first available event after buffer overflow
    // It must NOT be event 1 (which was dropped because it's oldest).
    // Instead it will jump forward to the retained recent window.
    let first_received = stream.next().await.expect("has item").expect("valid item");
    assert!(
        first_received.sequence > 1,
        "oldest events must be dropped first (received sequence {})",
        first_received.sequence
    );

    // Now emit one newest event
    uvc_backend.emit_observation(CameraStateCurrent::Recording, true, 1.0);

    // Drain remaining buffered items until we receive the newest event
    let mut saw_newest = false;
    for _ in 0..150 {
        if let Some(Ok(obs)) = stream.next().await {
            if obs.state == CameraStateCurrent::Recording {
                saw_newest = true;
                break;
            }
        }
    }
    assert!(
        saw_newest,
        "newest event must not be dropped by backpressure"
    );
}

#[tokio::test]
async fn test_reconciler_observation_push_and_monotonic_ordering() {
    let mut facade = Facade::new();
    let ptp_backend = Arc::new(PtpAdapter::new("cam-ptp-rec", "SN111"));
    let ptp_cfg = CameraConfig::new("cam-ptp-rec", "ptp", "SN111");
    let handle = facade.register(ptp_cfg, ptp_backend.clone());

    let mut stream = facade.observe(handle.id()).await.expect("stream");

    let mut cam_state = CameraState::new("cam-ptp-rec".into(), "PTP".into(), CameraType::Business);
    cam_state.set_desired(CameraDesired::Idle);

    // Reconciler handles observation directly
    ptp_backend.set_status(PtpStatus::Ready);
    let obs = stream.next().await.expect("item").expect("valid");
    assert_eq!(obs.state, CameraStateCurrent::Standby);

    let rem = facade
        .reconciler_mut()
        .handle_observation(&obs, &mut cam_state)
        .expect("monotonic observation must be processed");
    assert_eq!(rem.camera_id, "cam-ptp-rec");
    // With desired == Idle, the reconciler advances/reconciles Standby toward On (Idle desire target)
    assert_eq!(cam_state.state, CameraStateCurrent::On);
    assert!(cam_state.online);

    // Test out-of-order rejection: replay the same sequence number (or an older one)
    let duplicate_rem = facade
        .reconciler_mut()
        .handle_observation(&obs, &mut cam_state);
    assert!(
        duplicate_rem.is_none(),
        "out of order / duplicate sequence numbers must be rejected"
    );

    // Verify audit log recorded the push observation
    let audit_entries = facade.reconciler().audit().entries();
    assert!(
        audit_entries
            .iter()
            .any(|e| e.what.contains("observe [seq:")),
        "audit should reflect observe push event: {audit_entries:?}"
    );
}

#[tokio::test]
async fn test_reconciler_automatic_push_subscription() {
    let mut facade = Facade::new();
    let ptp_backend = Arc::new(PtpAdapter::new("cam-ptp-sub", "SN222"));
    let ptp_cfg = CameraConfig::new("cam-ptp-sub", "ptp", "SN222");
    let handle = facade.register(ptp_cfg, ptp_backend.clone());

    let stream = facade.observe(handle.id()).await.expect("observe stream");
    let reconciler = Arc::new(tokio::sync::Mutex::new(cctv_agent::Reconciler::default()));
    reconciler
        .lock()
        .await
        .set_desired("cam-ptp-sub", CameraDesired::Idle);

    let cam_state = Arc::new(tokio::sync::Mutex::new(CameraState::new(
        "cam-ptp-sub".into(),
        "PTP".into(),
        CameraType::Business,
    )));

    // Spawn background reconciler subscriber
    let task = Facade::subscribe_reconciler_to_camera(
        Arc::clone(&reconciler),
        stream,
        Arc::clone(&cam_state),
    )
    .await;

    // Trigger state change
    ptp_backend.set_status(PtpStatus::Ready);

    // Wait briefly for push event to be consumed by the reconciler
    tokio::time::sleep(Duration::from_millis(50)).await;

    let state_snapshot = cam_state.lock().await.clone();
    assert_eq!(state_snapshot.state, CameraStateCurrent::On);
    assert!(state_snapshot.online);

    let rec_guard = reconciler.lock().await;
    assert!(
        rec_guard
            .audit()
            .entries()
            .iter()
            .any(|e| e.what.contains("observe [seq:")),
        "audit trail must reflect automatic push observation"
    );

    task.abort();
}
