//! The facade against the mock device model: the registry, the command surface,
//! the driver registry and the fleet sweep.
//!
//! No network and no runtime — the mock's futures never wait on I/O, so a
//! `std`-only executor drives them.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use cctv_agent::Grade;
use cctv_core::mock::MockCamera;
use cctv_core::{
    BoxFuture, CameraBackend, CameraConfig, CameraDesired, CameraError, CameraStateCurrent,
    CommandId, CommandResultPayload, ProbeResult, PtzCommand, StreamInfo, StreamQuality,
};
use cctv_discovery::Datagram;
use cctv_rs::{Action, DriverFactory, Facade};

/// A `std`-only executor: the mock's futures never wait on I/O.
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

fn config(id: &str) -> CameraConfig {
    CameraConfig::new(id, "mock", "10.0.0.1")
}

fn facade_with(id: &str) -> Facade {
    let mut facade = Facade::new();
    facade.register(config(id), Arc::new(MockCamera::new()));
    facade
}

/// A camera whose every call fails, for the graded-remediation path.
struct DeadCamera;

impl CameraBackend for DeadCamera {
    fn driver(&self) -> &'static str {
        "dead"
    }

    fn probe(&self) -> BoxFuture<'_, Result<ProbeResult, CameraError>> {
        Box::pin(async { Err(CameraError::Offline) })
    }

    fn stream(&self, _quality: StreamQuality) -> BoxFuture<'_, Result<StreamInfo, CameraError>> {
        Box::pin(async { Err(CameraError::Offline) })
    }
}

/// A transport that answers nothing.
struct Silent;

impl Datagram for Silent {
    fn exchange(&self, _group: SocketAddr, _request: &[u8], _timeout: Duration) -> Vec<Vec<u8>> {
        Vec::new()
    }
}

#[test]
fn registering_a_camera_makes_it_listable_and_re_registration_replaces_it() {
    let mut facade = Facade::new();
    assert!(facade.list_cameras().is_empty());

    facade.register(config("b"), Arc::new(MockCamera::new()));
    facade.register(config("a"), Arc::new(MockCamera::new()));
    let ids: Vec<String> = facade
        .list_cameras()
        .iter()
        .map(|handle| handle.id().to_string())
        .collect();
    assert_eq!(ids, vec!["a", "b"], "ordered by id");

    facade.register(config("a"), Arc::new(MockCamera::new()));
    assert_eq!(facade.list_cameras().len(), 2, "replaced, not duplicated");

    assert_eq!(facade.unregister("a").expect("removed").id, "a");
    assert!(!facade.contains("a"));
    assert!(facade.unregister("a").is_none());
}

#[test]
fn a_command_on_an_unknown_camera_is_a_typed_error() {
    let facade = Facade::new();
    assert_eq!(
        facade.camera_config_get("nope").unwrap_err(),
        CameraError::NoCamera
    );
    assert_eq!(
        block_on(facade.get_camera_frame("nope")).unwrap_err(),
        CameraError::NoCamera
    );
    assert_eq!(
        block_on(facade.ptz_move("nope", PtzCommand::Stop)).unwrap_err(),
        CameraError::NoCamera
    );
    assert_eq!(
        block_on(facade.execute("nope", CommandId::CameraGetStats)).unwrap_err(),
        CameraError::NoCamera
    );
}

#[test]
fn camera_config_get_redacts_the_password() {
    let mut facade = Facade::new();
    facade.register(
        CameraConfig::new("cam", "mock", "10.0.0.1").with_credentials("admin", "hunter2"),
        Arc::new(MockCamera::new()),
    );

    let redacted = facade.camera_config_get("cam").expect("a config");
    assert_eq!(redacted.username.as_deref(), Some("admin"));
    assert_eq!(redacted.password.as_deref(), Some("***"));
    // The facade still holds the real one.
    assert_eq!(
        facade.config("cam").unwrap().password.as_deref(),
        Some("hunter2")
    );
}

#[test]
fn camera_config_set_replaces_the_record() {
    let mut facade = facade_with("cam");
    let mut updated = config("cam");
    updated.label = Some("Front Door".into());
    facade.camera_config_set(updated).expect("updated");
    assert_eq!(
        facade.config("cam").unwrap().label.as_deref(),
        Some("Front Door")
    );

    assert_eq!(
        facade.camera_config_set(config("nope")).unwrap_err(),
        CameraError::NoCamera
    );
}

#[test]
fn get_camera_frame_returns_the_image_bytes() {
    let facade = facade_with("cam");
    let frame = block_on(facade.get_camera_frame("cam")).expect("a frame");
    assert!(
        !frame.bytes.is_empty(),
        "the image travels here, not in the payload"
    );
    assert_eq!(frame.content_type, "image/jpeg");
}

#[test]
fn start_camera_stream_returns_the_device_model_value() {
    let facade = facade_with("cam");
    let stream = block_on(facade.start_camera_stream("cam", StreamQuality::Sub)).expect("a stream");
    assert_eq!(stream.quality, StreamQuality::Sub);
    assert_eq!(stream.uri, "rtsp://127.0.0.1/sub");
    assert!(stream.uri.contains("://"));
}

#[test]
fn ptz_move_reaches_the_backend() {
    let mut facade = Facade::new();
    let camera = Arc::new(MockCamera::new());
    facade.register(config("cam"), camera.clone());

    let command = PtzCommand::Continuous {
        pan: 0.5,
        tilt: 0.0,
        zoom: 0.0,
    };
    block_on(facade.ptz_move("cam", command)).expect("moved");
    block_on(facade.ptz_move("cam", PtzCommand::Stop)).expect("stopped");

    let log = camera.ptz_log.lock().expect("lock");
    assert_eq!(log.as_slice(), &[command, PtzCommand::Stop]);
}

#[test]
fn presets_round_trip_through_the_facade() {
    let facade = facade_with("cam");
    assert!(block_on(facade.camera_get_presets("cam"))
        .expect("listed")
        .is_empty());

    let saved = block_on(facade.camera_save_preset("cam", "Door")).expect("saved");
    assert_eq!(saved.name, "Door");
    assert!(!saved.id.is_empty());

    block_on(facade.camera_goto_preset("cam", &saved.id)).expect("moved");
    assert_eq!(
        block_on(facade.camera_get_presets("cam"))
            .expect("listed")
            .len(),
        1
    );

    block_on(facade.camera_delete_preset("cam", &saved.id)).expect("deleted");
    assert!(block_on(facade.camera_get_presets("cam"))
        .expect("listed")
        .is_empty());

    // A preset that was never stored is a typed error, not a panic.
    assert!(block_on(facade.camera_goto_preset("cam", "ghost")).is_err());
}

#[test]
fn the_health_and_stats_commands_come_back_as_catalogue_results() {
    let facade = facade_with("cam");

    let health = block_on(facade.test_camera_connection("cam")).expect("a result");
    assert_eq!(health.command, CommandId::TestCameraConnection);
    assert_eq!(health.camera_id.as_deref(), Some("cam"));
    match health.result {
        CommandResultPayload::Health { healthy, detail } => {
            assert!(healthy);
            assert!(detail.contains("Mock"), "{detail}");
        }
        other => panic!("unexpected payload: {other:?}"),
    }

    let monitored = block_on(facade.monitor_camera_health("cam")).expect("a result");
    match monitored.result {
        CommandResultPayload::Health { healthy, detail } => {
            assert!(healthy);
            assert!(detail.contains("rtsp://127.0.0.1/main"), "{detail}");
        }
        other => panic!("unexpected payload: {other:?}"),
    }

    let stats = block_on(facade.camera_get_stats("cam")).expect("a result");
    match stats.result {
        CommandResultPayload::Stats { online, fps, .. } => {
            assert!(online);
            assert_eq!(fps, 0.0, "nothing measures frame rate yet");
        }
        other => panic!("unexpected payload: {other:?}"),
    }

    let reconnection = block_on(facade.camera_get_reconnection_info("cam")).expect("a result");
    match reconnection.result {
        CommandResultPayload::Reconfiguration { info } => {
            assert!(info.starts_with("reachable:"), "{info}");
            assert!(info.contains("mock"), "{info}");
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn the_fleet_commands_cover_every_registered_camera() {
    let mut facade = Facade::new();
    facade.register(config("a"), Arc::new(MockCamera::new()));
    facade.register(config("b"), Arc::new(MockCamera::new()));
    facade.register(config("dead"), Arc::new(DeadCamera));

    let test = block_on(facade.camera_test_all()).expect("a result");
    match test.result {
        CommandResultPayload::Test { passed, detail } => {
            assert!(!passed, "one camera is down");
            assert!(detail.contains("dead"), "{detail}");
        }
        other => panic!("unexpected payload: {other:?}"),
    }

    let stats = block_on(facade.camera_get_all_stats()).expect("a result");
    match stats.result {
        CommandResultPayload::AllStats { cameras } => {
            let ids: Vec<&str> = cameras.iter().map(|camera| camera.id.as_str()).collect();
            assert_eq!(ids, vec!["a", "b", "dead"]);
            let dead = cameras.iter().find(|camera| camera.id == "dead").unwrap();
            assert!(!dead.online);
            assert_eq!(dead.vendor, "", "an offline camera reports no vendor");
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn execute_routes_the_catalogue_and_refuses_the_parameterised_commands() {
    let facade = facade_with("cam");

    let config = block_on(facade.execute("cam", CommandId::CameraConfigGet)).expect("a result");
    assert_eq!(config.command, CommandId::CameraConfigGet);
    assert!(matches!(config.result, CommandResultPayload::Config { .. }));

    let presets = block_on(facade.execute("cam", CommandId::CameraGetPresets)).expect("a result");
    assert!(matches!(
        presets.result,
        CommandResultPayload::Presets { .. }
    ));

    // Parameterised commands have typed methods; the generic entry point says so
    // rather than silently doing nothing.
    assert_eq!(
        block_on(facade.execute("cam", CommandId::PtzMove)).unwrap_err(),
        CameraError::NotImplemented
    );
}

#[test]
fn the_driver_registry_builds_a_backend_from_a_record() {
    let mut facade = Facade::new();
    assert!(facade.driver_keys().is_empty());

    let factory: DriverFactory =
        Arc::new(
            |_config: &CameraConfig| Ok(Arc::new(MockCamera::new()) as Arc<dyn CameraBackend>),
        );
    facade.register_driver("mock", factory);
    assert_eq!(facade.driver_keys(), vec!["mock".to_string()]);

    let handle = facade.attach(config("cam")).expect("attached");
    assert_eq!(handle.id(), "cam");
    assert!(facade.contains("cam"));

    // An unknown key is a typed error, not a panic.
    let unknown = CameraConfig::new("x", "nope", "10.0.0.1");
    assert!(matches!(facade.attach(unknown), Err(CameraError::Other(_))));
}

#[test]
fn the_builtin_registry_names_the_ezviz_driver() {
    let facade = Facade::new().with_builtin_drivers();
    assert_eq!(
        facade.driver_keys(),
        vec!["ezviz".to_string(), "onvif".to_string()]
    );
}

#[test]
fn discovery_reports_cameras_without_registering_them() {
    let facade = Facade::new();
    assert!(facade.discover_with(&Silent).is_empty());
    assert!(
        facade.list_cameras().is_empty(),
        "discovery reports; registration is the application's choice"
    );
}

#[test]
fn reconciling_advances_the_fleet_and_records_every_remediation() {
    let mut facade = Facade::new();
    facade.register(config("a"), Arc::new(MockCamera::new()));
    facade.register(config("dead"), Arc::new(DeadCamera));
    facade
        .reconciler_mut()
        .set_desired("a", CameraDesired::Armed);
    facade
        .reconciler_mut()
        .set_desired("dead", CameraDesired::Armed);

    let remediations = block_on(facade.reconcile());
    assert_eq!(remediations.len(), 2);

    let healthy = remediations
        .iter()
        .find(|r| r.camera_id == "a")
        .expect("a remediation");
    assert_eq!(
        healthy.action,
        Action::Advance(CameraStateCurrent::Streaming)
    );
    assert_eq!(healthy.grade, Grade::Notify);

    let dead = remediations
        .iter()
        .find(|r| r.camera_id == "dead")
        .expect("a remediation");
    assert_eq!(dead.grade, Grade::Retry, "unreachable earns a retry");
    assert_eq!(dead.action, Action::MarkOffline);

    assert_eq!(facade.reconciler().audit().len(), 2, "all on the record");
}

#[test]
fn disabling_a_camera_sets_the_intent_and_reconciles_it_off() {
    let mut facade = facade_with("cam");
    facade.camera_set_enabled("cam", false).expect("disabled");
    assert_eq!(
        facade.reconciler().desired_for("cam"),
        Some(CameraDesired::Disabled)
    );

    // The camera is online and streaming, so taking it out of service is work.
    let remediations = block_on(facade.reconcile());
    assert_eq!(remediations[0].grade, Grade::Reconfigure);
    assert_eq!(remediations[0].action, Action::TakeOutOfService);

    assert_eq!(
        facade.camera_set_enabled("nope", true).unwrap_err(),
        CameraError::NoCamera
    );
}
