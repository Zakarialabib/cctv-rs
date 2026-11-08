//! Host integration test for the cctv-rs facade and mock camera.

use std::future::Future;
use std::sync::Arc;

use cctv_core::mock::MockCamera;
use cctv_rs::{
    CameraConfig, CommandId, CommandResultPayload, Detection, Facade, PtzCommand, StreamMux,
    StreamMuxConfig, StreamQuality, Zone, ZoneConfig,
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
fn host_integration_test() {
    let mut facade = Facade::new();
    let cam_id = "test-cam";

    // Register the mock camera directly
    let config = CameraConfig::new(cam_id, "mock", "10.0.0.1");
    let mock = MockCamera::new();
    facade.register(config, Arc::new(mock));

    block_on(async {
        // 1. Start a stream through the driver seam
        let stream_info = facade
            .start_camera_stream(cam_id, StreamQuality::Main)
            .await
            .expect("start stream");
        assert_eq!(stream_info.quality, StreamQuality::Main);

        // 2. Read frames through the stream and StreamMux path
        let mux_config = StreamMuxConfig::new(&stream_info.uri);
        let mux = StreamMux::new(mux_config);

        let _sub = mux.subscribe().await;

        // 3. Issue PTZ move plus preset save
        facade
            .ptz_move(
                cam_id,
                PtzCommand::Continuous {
                    pan: 1.0,
                    tilt: 0.0,
                    zoom: 0.0,
                },
            )
            .await
            .expect("ptz move");

        let preset = facade
            .camera_save_preset(cam_id, "door_view")
            .await
            .expect("save preset");
        assert_eq!(preset.name, "door_view");

        // 4. Snapshot commands
        let snapshot = facade.get_camera_frame(cam_id).await.expect("snapshot");
        assert_eq!(snapshot.content_type, "image/jpeg");

        // 5. Record a detection zone event
        facade.zone_save(
            "zone1",
            ZoneConfig::default_for(Zone::point(0.5, 0.5, 0.1), 0.5),
        );
        let detection = Detection::motion(Zone::point(0.5, 0.5, 0.1), 0.9);

        let _event = facade
            .record_detection(cam_id, "zone1", &detection)
            .expect("record detection");

        // Let's verify event list
        let result = facade
            .execute(cam_id, CommandId::ZoneMotionListEvents)
            .await
            .expect("list events");
        match result.result {
            CommandResultPayload::MotionEvents { events } => {
                assert!(!events.is_empty(), "should have events");
            }
            _ => panic!("expected MotionEvents"),
        }
    });
}
