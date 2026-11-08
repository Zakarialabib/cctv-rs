//! Facade-level integration test that exercises the host-facing surface end to end with the mock driver.

use cctv_core::mock::MockCamera;
use cctv_core::{PtzCommand, StreamQuality};
use cctv_rs::{CameraConfig, Detection, Facade, Zone, ZoneConfig};

#[tokio::test]
async fn test_host_contract() {
    let mut facade = Facade::new();

    // Attach camera using MockCamera as backend (it implements the missing methods)
    let config = CameraConfig::new("cam-1", "mock", "10.0.0.1");
    facade.register_backend(config, Box::new(MockCamera::new()));

    // Check stream
    let stream = facade
        .start_camera_stream("cam-1", StreamQuality::Sub)
        .await
        .expect("stream should succeed");
    assert_eq!(stream.uri, "rtsp://127.0.0.1/sub"); // MockCamera uses this

    // Push some frames through the stream path (StreamMux)
    let reader = facade.frame_reader("cam-1").expect("a reader");
    let now = cctv_rs::clock_ms();
    {
        let mut reader = reader.lock().expect("lock");
        reader.set_window(1_000);
        for i in 0..30u64 {
            reader.push(
                1920,
                1080,
                vec![0u8; 100],
                now.saturating_sub(1_000) + i * 33,
            );
        }
    }

    // Check PTZ
    facade
        .ptz_move("cam-1", PtzCommand::Stop)
        .await
        .expect("ptz should succeed");

    // Check preset
    facade
        .camera_save_preset("cam-1", "my-preset")
        .await
        .expect("preset should succeed");

    // Check snapshot
    facade
        .get_camera_frame("cam-1")
        .await
        .expect("snapshot should succeed");

    // Check zone
    let zone = Zone::rectangle(0.25, 0.25, 0.5, 0.5);
    let config = ZoneConfig::default_for(zone, 0.7);
    facade.zone_save("door", config.clone());

    let loud = Detection::motion(Zone::point(0.0, 0.0, 4.0), 0.9);
    facade
        .record_detection("cam-1", "door", &loud)
        .expect("zone exists");
}
