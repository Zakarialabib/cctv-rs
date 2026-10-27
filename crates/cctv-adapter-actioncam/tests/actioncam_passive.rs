//! Passive discovery and status inspection tests for Insta360 X4.

use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use cctv_adapter_actioncam::{identify_insta360_x4, ActionCamClient, ActionCamStatus};

#[test]
fn test_identify_insta360_x4_via_mdns_txt() {
    let mut txt_map = HashMap::new();
    txt_map.insert("manufacturer".to_string(), "Insta360".to_string());
    txt_map.insert("model".to_string(), "Insta360 X4".to_string());
    txt_map.insert("serial".to_string(), "X4-99887766".to_string());
    txt_map.insert("api_version".to_string(), "v1.0".to_string());

    let identity =
        identify_insta360_x4(&txt_map, "192.168.1.150", 8080).expect("Should identify Insta360 X4");

    assert_eq!(identity.model, "Insta360 X4");
    assert_eq!(identity.manufacturer, "Insta360");
    assert_eq!(identity.serial_number.as_deref(), Some("X4-99887766"));
    assert_eq!(identity.api_version.as_deref(), Some("v1.0"));
    assert_eq!(identity.ip, "192.168.1.150");
    assert_eq!(identity.port, 8080);
}

#[tokio::test]
async fn test_fetch_status_without_sending_command_bytes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0u8; 2048];
        let bytes_read = stream.read(&mut buffer).await.unwrap();
        let request_str = String::from_utf8_lossy(&buffer[..bytes_read]);

        // Verify that only HTTP GET was issued (passive request)
        assert!(
            request_str.starts_with("GET /status"),
            "Expected GET /status, got: {request_str}"
        );

        // Verify no command body was attached to the request
        let header_end = request_str
            .find("\r\n\r\n")
            .expect("Request headers delimiter");
        let body_bytes = &buffer[header_end + 4..bytes_read];
        assert_eq!(
            body_bytes.len(),
            0,
            "No command bytes should be sent in body during status fetch"
        );

        let json_body = r#"{
            "battery_level": 92,
            "recording_state": "idle",
            "storage_remaining_bytes": 128000000000,
            "temperature_celsius": 34.2,
            "capture_mode": "video_360",
            "firmware_version": "v1.0.8"
        }"#;

        let http_response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            json_body.len(),
            json_body
        );

        stream.write_all(http_response.as_bytes()).await.unwrap();
    });

    let client = ActionCamClient::new(&format!("http://{addr}"));
    let status = client
        .fetch_status()
        .await
        .expect("Fetch status should succeed");

    assert_eq!(
        status,
        ActionCamStatus {
            battery_level: 92,
            recording_state: "idle".to_string(),
            storage_remaining_bytes: 128_000_000_000,
            temperature_celsius: Some(34.2),
            capture_mode: "video_360".to_string(),
            firmware_version: Some("v1.0.8".to_string()),
        }
    );

    server_task.await.unwrap();
}
