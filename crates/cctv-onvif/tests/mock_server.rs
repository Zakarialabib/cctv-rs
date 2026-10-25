//! The ONVIF session and backend against a canned SOAP server: the conformance
//! suite plus the device / media / PTZ calls, with no camera and no network.
//!
//! `cctv-transport`'s fixture server matches a route by method, path and a
//! substring of the SOAP body, so each service gets one route per action — which
//! is also what proves the builders put the action name where a device expects it.

use std::time::Duration;

use cctv_core::conformance::{run, Expectations};
use cctv_core::{CameraBackend, PtzCommand, StreamQuality};
use cctv_onvif::{Credentials, OnvifCameraBackend, OnvifSession};
use cctv_transport::fixture::{FixtureServer, Route};
use cctv_transport::http::HttpConfig;

/// A minimal JPEG carrying a SOF0 frame header, so the dimensions parse.
const JPEG: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01,
    0x00, 0x01, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x0F, 0x00, 0x14, 0x01, 0x01, 0x11,
    0x00, 0xFF, 0xD9,
];

const DEVICE_INFO: &str = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope" xmlns:tds="http://www.onvif.org/ver10/device/wsdl"><SOAP-ENV:Body><tds:GetDeviceInformationResponse>
  <tds:Manufacturer>ACME</tds:Manufacturer><tds:Model>PTZ-9000</tds:Model>
  <tds:FirmwareVersion>V5.6.3</tds:FirmwareVersion><tds:SerialNumber>SN123</tds:SerialNumber>
</tds:GetDeviceInformationResponse></SOAP-ENV:Body></SOAP-ENV:Envelope>"#;

const CAPABILITIES: &str = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope"><SOAP-ENV:Body>
<tds:GetCapabilitiesResponse><tds:Capabilities>
  <tt:Media><tt:XAddr>{{base}}/onvif/media</tt:XAddr></tt:Media>
  <tt:PTZ><tt:XAddr>{{base}}/onvif/ptz</tt:XAddr></tt:PTZ>
</tds:Capabilities></tds:GetCapabilitiesResponse></SOAP-ENV:Body></SOAP-ENV:Envelope>"#;

const PROFILES: &str = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope"><SOAP-ENV:Body>
<trt:GetProfilesResponse>
  <trt:Profiles token="profile_1" fixed="true"><tt:Name>MainStream</tt:Name>
    <tt:VideoEncoderConfiguration token="venc_1"><tt:Encoding>H264</tt:Encoding>
      <tt:Resolution><tt:Width>1920</tt:Width><tt:Height>1080</tt:Height></tt:Resolution>
    </tt:VideoEncoderConfiguration>
    <tt:PTZConfiguration token="ptz_1"><tt:Name>PTZ_1</tt:Name></tt:PTZConfiguration>
  </trt:Profiles>
  <trt:Profiles token="profile_2" fixed="true"><tt:Name>SubStream</tt:Name>
    <tt:VideoEncoderConfiguration token="venc_2"><tt:Encoding>H264</tt:Encoding>
      <tt:Resolution><tt:Width>640</tt:Width><tt:Height>360</tt:Height></tt:Resolution>
    </tt:VideoEncoderConfiguration>
  </trt:Profiles>
</trt:GetProfilesResponse></SOAP-ENV:Body></SOAP-ENV:Envelope>"#;

const STREAM_URI: &str = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope"><SOAP-ENV:Body>
<trt:GetStreamUriResponse><trt:MediaUri>
  <tt:Uri>rtsp://cam.local:554/Streaming/Channels/101</tt:Uri>
</trt:MediaUri></trt:GetStreamUriResponse></SOAP-ENV:Body></SOAP-ENV:Envelope>"#;

const SNAPSHOT_URI: &str = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope"><SOAP-ENV:Body>
<trt:GetSnapshotUriResponse><trt:MediaUri>
  <tt:Uri>{{base}}/snapshot.jpg</tt:Uri>
</trt:MediaUri></trt:GetSnapshotUriResponse></SOAP-ENV:Body></SOAP-ENV:Envelope>"#;

const PRESETS: &str = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope"><SOAP-ENV:Body>
<tptz:GetPresetsResponse>
  <tptz:Preset token="1"><tt:Name>Door</tt:Name></tptz:Preset>
</tptz:GetPresetsResponse></SOAP-ENV:Body></SOAP-ENV:Envelope>"#;

const SET_PRESET: &str = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope"><SOAP-ENV:Body>
<tptz:SetPresetResponse><tptz:PresetToken>preset-3</tptz:PresetToken></tptz:SetPresetResponse></SOAP-ENV:Body></SOAP-ENV:Envelope>"#;

const ACK: &str = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope"><SOAP-ENV:Body/></SOAP-ENV:Envelope>"#;

const FAULT: &str = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope"><SOAP-ENV:Body>
<SOAP-ENV:Fault><SOAP-ENV:Code><SOAP-ENV:Value>SOAP-ENV:Receiver</SOAP-ENV:Value></SOAP-ENV:Code>
<SOAP-ENV:Reason><SOAP-ENV:Text xml:lang="en">Sender not Authorized</SOAP-ENV:Text></SOAP-ENV:Reason>
</SOAP-ENV:Fault></SOAP-ENV:Body></SOAP-ENV:Envelope>"#;

fn routes() -> Vec<Route> {
    vec![
        Route::soap("/onvif/device_service", "GetDeviceInformation", DEVICE_INFO),
        Route::soap("/onvif/device_service", "GetCapabilities", CAPABILITIES),
        Route::soap("/onvif/broken", "GetDeviceInformation", FAULT),
        Route::soap("/onvif/media", "GetProfiles", PROFILES),
        Route::soap("/onvif/media", "GetStreamUri", STREAM_URI),
        Route::soap("/onvif/media", "GetSnapshotUri", SNAPSHOT_URI),
        Route::soap("/onvif/ptz", "ContinuousMove", ACK),
        Route::soap("/onvif/ptz", "AbsoluteMove", ACK),
        Route::soap("/onvif/ptz", "Stop", ACK),
        Route::soap("/onvif/ptz", "GetPresets", PRESETS),
        Route::soap("/onvif/ptz", "SetPreset", SET_PRESET),
        Route::soap("/onvif/ptz", "GotoPreset", ACK),
        Route::soap("/onvif/ptz", "RemovePreset", ACK),
        Route::bytes("GET", "/snapshot.jpg", "image/jpeg", JPEG.to_vec()),
    ]
}

/// A session pointed at the fixture's device service.
fn session_at(server: &FixtureServer, path: &str) -> OnvifSession {
    OnvifSession::new(
        format!("{}{path}", server.url()),
        Credentials {
            username: "admin".into(),
            password: "hunter2".into(),
        },
        HttpConfig {
            timeout: Duration::from_secs(5),
            ..HttpConfig::default()
        },
    )
    .expect("a session")
}

fn backend(server: &FixtureServer) -> OnvifCameraBackend {
    OnvifCameraBackend::new(session_at(server, "/onvif/device_service"))
}

#[tokio::test]
async fn the_backend_passes_the_conformance_suite() {
    let server = FixtureServer::start(routes()).await.expect("a server");
    let backend = backend(&server);
    let report = run(&backend, Expectations::FULL).await;
    assert!(report.is_ok(), "{report}");
}

#[tokio::test]
async fn the_session_reads_identity_and_capabilities() {
    let server = FixtureServer::start(routes()).await.expect("a server");
    let session = session_at(&server, "/onvif/device_service");

    let info = session.device_information().await.expect("identity");
    assert_eq!(info.manufacturer, "ACME");
    assert_eq!(info.model, "PTZ-9000");
    assert_eq!(info.firmware_version.as_deref(), Some("V5.6.3"));

    // The XAddrs the device names are not its device-service URL, so the session
    // must follow them.
    let services = session.services().await.expect("capabilities");
    assert!(services
        .media
        .as_deref()
        .is_some_and(|u| u.ends_with("/onvif/media")));
    assert!(services
        .ptz
        .as_deref()
        .is_some_and(|u| u.ends_with("/onvif/ptz")));

    // Cached: a second call does not re-ask.
    let before = server
        .requests()
        .iter()
        .filter(|r| r.body.contains("GetCapabilities"))
        .count();
    session.services().await.expect("capabilities");
    let after = server
        .requests()
        .iter()
        .filter(|r| r.body.contains("GetCapabilities"))
        .count();
    assert_eq!((before, after), (1, 1), "the service table is cached");
}

#[tokio::test]
async fn the_session_resolves_the_main_and_sub_streams() {
    let server = FixtureServer::start(routes()).await.expect("a server");
    let backend = backend(&server);

    let main = backend.stream(StreamQuality::Main).await.expect("main");
    assert_eq!(main.profile, "profile_1");
    assert_eq!(main.uri, "rtsp://cam.local:554/Streaming/Channels/101");
    assert_eq!((main.width, main.height), (Some(1920), Some(1080)));
    assert_eq!(main.codec.as_deref(), Some("H264"));

    // The sub stream is the smallest profile, not the first one.
    let sub = backend.stream(StreamQuality::Sub).await.expect("sub");
    assert_eq!(sub.profile, "profile_2");
    assert_eq!((sub.width, sub.height), (Some(640), Some(360)));
}

#[tokio::test]
async fn the_snapshot_is_fetched_from_the_device() {
    let server = FixtureServer::start(routes()).await.expect("a server");
    let snapshot = backend(&server).snapshot().await.expect("a snapshot");

    assert_eq!(snapshot.content_type, "image/jpeg");
    assert_eq!(snapshot.bytes, JPEG);
    // GetSnapshotUri returns a URL, so the bytes come from a second request.
    let gets = server
        .requests()
        .iter()
        .filter(|r| r.method == "GET" && r.target == "/snapshot.jpg")
        .count();
    assert_eq!(gets, 1);
}

#[tokio::test]
async fn stop_names_the_profile_that_moved() {
    let server = FixtureServer::start(routes()).await.expect("a server");
    let backend = backend(&server);

    backend
        .ptz(PtzCommand::Continuous {
            pan: 0.5,
            tilt: -0.5,
            zoom: 0.0,
        })
        .await
        .expect("moved");
    backend.ptz(PtzCommand::Stop).await.expect("stopped");

    let stop = server
        .requests()
        .into_iter()
        .find(|r| r.body.contains("Stop"))
        .expect("a stop was sent");
    assert!(
        stop.body
            .contains("<tptz:ProfileToken>profile_1</tptz:ProfileToken>"),
        "{}",
        stop.body
    );
    assert!(stop.body.contains("<tptz:PanTilt>true</tptz:PanTilt>"));
}

#[tokio::test]
async fn presets_round_trip_through_the_backend() {
    let server = FixtureServer::start(routes()).await.expect("a server");
    let backend = backend(&server);

    let listed = backend.presets().await.expect("listed");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "1");
    assert_eq!(listed[0].name, "Door");

    let saved = backend.save_preset("Gate", None).await.expect("saved");
    assert_eq!(saved.id, "preset-3", "the token the device assigned");
    assert_eq!(saved.name, "Gate");

    backend.goto_preset("preset-3").await.expect("moved");
    backend.delete_preset("preset-3").await.expect("deleted");
}

#[tokio::test]
async fn a_soap_fault_becomes_a_typed_error() {
    let server = FixtureServer::start(routes()).await.expect("a server");
    let error = session_at(&server, "/onvif/broken")
        .device_information()
        .await
        .expect_err("a fault");
    match error {
        cctv_onvif::OnvifError::Fault(reason) => {
            assert!(reason.contains("Authorized"), "{reason}")
        }
        other => panic!("unexpected error: {other}"),
    }
}
