//! The Ezviz driver against a canned HTTP server: the conformance suite plus the
//! account-level operations, with no camera, no credentials and no network.
//!
//! `cctv-transport`'s fixture server answers each `/api/lapp/**` call, and its
//! `{{base}}` placeholder lets the token response name the fixture as the
//! region's `areaDomain` — so the client's region switch is exercised for real.

use cctv_core::conformance::{run, Expectations};
use cctv_core::{CameraBackend, CameraError, PtzCommand, StreamQuality};
use cctv_ezviz::{Connectivity, Driver, DriverError, EzvizCameraBackend, EzvizClient};
use cctv_transport::fixture::{FixtureServer, Route};

/// A `200` JSON route for a form `POST`.
fn json(path: &str, body: &str) -> Route {
    Route::bytes("POST", path, "application/json", body.as_bytes().to_vec())
}

const TOKEN: &str = r#"{"code":"200","msg":"ok","data":{"accessToken":"tok-123","expireTime":1799999999000,"areaDomain":"{{base}}"}}"#;
const DEVICE_INFO: &str = r#"{"code":"200","msg":"ok","data":{"deviceSerial":"CAM1","deviceName":"Front Door","model":"CS-C6N","status":1,"defence":1,"version":"V5.3.0","ptz":true}}"#;
const DEVICE_LIST: &str = r#"{"code":"200","msg":"ok","page":{"total":1,"page":0,"size":50},"data":[{"deviceSerial":"CAM1","deviceName":"Front Door","deviceType":"C6","status":1,"defence":1,"deviceVersion":"V5.3.0","model":"CS-C6N"}]}"#;
const CAMERA_LIST: &str = r#"{"code":"200","msg":"ok","data":[{"deviceSerial":"CAM1","ipcSerial":"IPC1","channelNo":1,"channelName":"Front Door","status":1,"picUrl":"http://cam.local/pic.jpg"}]}"#;
const LIVE: &str = r#"{"code":"200","msg":"ok","data":{"id":"addr-1","url":"rtsp://ezviz.local/live/CAM1_1.m3u8","expireTime":"2026-11-01 00:00:00"}}"#;
const ACK: &str = r#"{"code":"200","msg":"ok"}"#;
const PRESET_ADD: &str = r#"{"code":"200","msg":"ok","data":{"index":3}}"#;
const ALARMS: &str = r#"{"code":"200","msg":"ok","data":[{"alarmId":"A1","alarmName":"Motion","alarmType":10002,"alarmTime":1799999999000,"channelNo":1,"isChecked":0,"deviceSerial":"CAM1","alarmPicUrl":"http://cam.local/a.jpg"}]}"#;

async fn server() -> FixtureServer {
    FixtureServer::start(vec![
        json("/api/lapp/token/get", TOKEN),
        json("/api/lapp/device/info", DEVICE_INFO),
        json("/api/lapp/device/list", DEVICE_LIST),
        json("/api/lapp/camera/list", CAMERA_LIST),
        json("/api/lapp/live/address/get", LIVE),
        json("/api/lapp/device/ptz/start", ACK),
        json("/api/lapp/device/ptz/stop", ACK),
        json("/api/lapp/device/preset/add", PRESET_ADD),
        json("/api/lapp/device/preset/move", ACK),
        json("/api/lapp/device/preset/clear", ACK),
        json("/api/lapp/alarm/list", ALARMS),
        json("/api/lapp/device/capture", ACK),
    ])
    .await
    .expect("the fixture server starts")
}

/// A client that already holds a token and points at the fixture.
fn client_for(server: &FixtureServer) -> EzvizClient {
    EzvizClient::with_token("tok-123", Some(server.url()))
}

#[tokio::test]
async fn discover_joins_devices_and_channels() {
    let server = server().await;
    let cameras = Driver::new(client_for(&server))
        .discover()
        .await
        .expect("discovery succeeds");

    assert_eq!(cameras.len(), 1);
    let camera = &cameras[0];
    assert_eq!(camera.id, "IPC1:1", "the channel qualifies the identity");
    assert_eq!(camera.label, "Front Door");
    assert_eq!(camera.firmware.as_deref(), Some("V5.3.0"));
    assert_eq!(camera.connectivity, Connectivity::Cloud);
    assert_eq!(
        camera.stream_url, None,
        "play addresses expire; none is cached"
    );
}

#[tokio::test]
async fn discovery_mints_a_token_and_follows_the_area_domain() {
    let server = server().await;
    // No token held: the client must mint one and honour the region it names.
    let client = EzvizClient::new("app-key", "app-secret");
    client.set_base_url(server.url());

    let cameras = Driver::new(client)
        .discover()
        .await
        .expect("discovery succeeds");
    assert_eq!(cameras.len(), 1);

    let targets: Vec<String> = server.requests().iter().map(|r| r.target.clone()).collect();
    assert!(
        targets.iter().any(|t| t == "/api/lapp/token/get"),
        "the token endpoint is used: {targets:?}"
    );
    assert!(
        targets.iter().any(|t| t == "/api/lapp/device/list"),
        "{targets:?}"
    );
}

#[tokio::test]
async fn alarms_are_reduced_to_the_device_model() {
    let server = server().await;
    let alarms = Driver::new(client_for(&server))
        .alarms(None, 20)
        .await
        .expect("alarms succeed");

    assert_eq!(alarms.len(), 1);
    assert_eq!(alarms[0].id, "A1");
    assert_eq!(alarms[0].device_serial, "CAM1");
    assert_eq!(alarms[0].channel, 1);
    assert_eq!(alarms[0].kind, 10002);
    assert_eq!(alarms[0].at_ms, 1_799_999_999_000);
    assert!(!alarms[0].read);
    assert_eq!(
        alarms[0].snapshot_url.as_deref(),
        Some("http://cam.local/a.jpg")
    );
}

#[tokio::test]
async fn capture_issues_the_request() {
    let server = server().await;
    Driver::new(client_for(&server))
        .capture("CAM1", 1)
        .await
        .expect("capture is accepted");

    let body = server
        .requests()
        .into_iter()
        .find(|r| r.target == "/api/lapp/device/capture")
        .map(|r| r.body)
        .unwrap_or_default();
    assert!(body.contains("deviceSerial=CAM1"), "{body}");
    assert!(body.contains("channelNo=1"), "{body}");
}

#[tokio::test]
async fn the_backend_passes_the_conformance_suite() {
    let server = server().await;
    let backend = EzvizCameraBackend::new(client_for(&server), "CAM1", 1);
    let expect = Expectations {
        // The platform cannot return image bytes, and cannot list presets.
        snapshot: false,
        ptz: true,
        presets: false,
    };

    let report = run(&backend, expect).await;
    assert!(report.is_ok(), "{report}");
    assert_eq!(report.passed.len(), 7);
}

#[tokio::test]
async fn the_backend_resolves_a_stream_and_moves_the_camera() {
    let server = server().await;
    let backend = EzvizCameraBackend::new(client_for(&server), "CAM1", 1);

    let stream = backend.stream(StreamQuality::Main).await.expect("a stream");
    assert_eq!(stream.uri, "rtsp://ezviz.local/live/CAM1_1.m3u8");
    assert_eq!(stream.quality, StreamQuality::Main);
    assert_eq!(stream.profile, "1");

    backend
        .ptz(PtzCommand::Continuous {
            pan: 1.0,
            tilt: 0.0,
            zoom: 0.0,
        })
        .await
        .expect("move right");
    backend.ptz(PtzCommand::Stop).await.expect("stop");

    let requests = server.requests();
    let starts = requests
        .iter()
        .filter(|r| r.target == "/api/lapp/device/ptz/start")
        .count();
    let stops = requests
        .iter()
        .filter(|r| r.target == "/api/lapp/device/ptz/stop")
        .count();
    assert_eq!((starts, stops), (1, 1), "one start, one stop");

    // The platform is told which direction to stop, because it needs one.
    let stop = requests
        .into_iter()
        .find(|r| r.target == "/api/lapp/device/ptz/stop")
        .expect("a stop request");
    assert!(
        stop.body.contains("direction=3"),
        "right is 3: {}",
        stop.body
    );

    // An absolute move is not expressible on this platform.
    assert_eq!(
        backend
            .ptz(PtzCommand::Absolute {
                pan: 0.5,
                tilt: 0.0,
                zoom: 0.0
            })
            .await,
        Err(CameraError::NotImplemented)
    );
}

#[tokio::test]
async fn presets_save_recall_and_clear() {
    let server = server().await;
    let backend = EzvizCameraBackend::new(client_for(&server), "CAM1", 1);

    // The platform cannot list presets, and the driver says so with a typed error.
    assert_eq!(backend.presets().await, Err(CameraError::NotImplemented));

    let saved = backend.save_preset("Door", None).await.expect("save");
    assert_eq!(saved.id, "3", "the platform assigns the index");
    assert_eq!(saved.name, "Door");
    backend.goto_preset("3").await.expect("goto");
    backend.delete_preset("3").await.expect("delete");

    // A caller-chosen id is refused rather than silently ignored.
    assert_eq!(
        backend.save_preset("Door", Some("7")).await,
        Err(CameraError::NotImplemented)
    );
    // A non-numeric id is a typed error, not a panic.
    assert!(backend.goto_preset("front-door").await.is_err());
}

#[tokio::test]
async fn a_transport_failure_is_a_typed_error_not_a_panic() {
    // Nothing is listening on this port.
    let client = EzvizClient::with_token("tok-123", Some("http://127.0.0.1:1".to_string()));
    let error = Driver::new(client)
        .discover()
        .await
        .expect_err("no server is listening");

    assert!(
        matches!(error, DriverError::Transport(_)),
        "expected a transport failure, got {error:?}"
    );
    assert_eq!(CameraError::from(error), CameraError::Network);
}
