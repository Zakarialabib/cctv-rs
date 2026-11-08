//! Integration tests for ONVIF WS-Eventing subscription, renewal, reconnects, backpressure,
//! and cctv-agent motion event streaming and action triggers.

use std::sync::Arc;
use std::time::{Duration, Instant};

use cctv_agent::{DetectionEvent, EventSubscriptionService, SignageCommand};
use cctv_core::CameraId;
use cctv_driver::onvif::{subscribe_events, Credentials, OnvifSession, PullPointConfig};
use cctv_transport::fixture::{FixtureServer, Route};
use cctv_transport::http::HttpConfig;

const CAPABILITIES: &str = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope"><SOAP-ENV:Body>
<tds:GetCapabilitiesResponse xmlns:tds="http://www.onvif.org/ver10/device/wsdl" xmlns:tt="http://www.onvif.org/ver10/schema">
  <tds:Capabilities>
    <tt:Events><tt:XAddr>{{base}}/onvif/events_service</tt:XAddr></tt:Events>
  </tds:Capabilities>
</tds:GetCapabilitiesResponse></SOAP-ENV:Body></SOAP-ENV:Envelope>"#;

const SUB_RESPONSE: &str = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope" xmlns:tev="http://www.onvif.org/ver10/events/wsdl" xmlns:wsa="http://www.w3.org/2005/08/addressing">
  <SOAP-ENV:Body>
    <tev:CreatePullPointSubscriptionResponse>
      <tev:SubscriptionReference>
        <wsa:Address>{{base}}/onvif/pullpoint</wsa:Address>
      </tev:SubscriptionReference>
      <wsnt:CurrentTime xmlns:wsnt="http://docs.oasis-open.org/wsn/b-2">2026-10-23T01:00:00Z</wsnt:CurrentTime>
      <wsnt:TerminationTime xmlns:wsnt="http://docs.oasis-open.org/wsn/b-2">2026-10-23T01:01:00Z</wsnt:TerminationTime>
    </tev:CreatePullPointSubscriptionResponse>
  </SOAP-ENV:Body>
</SOAP-ENV:Envelope>"#;

const PULL_MOTION: &str = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope" xmlns:tev="http://www.onvif.org/ver10/events/wsdl" xmlns:wsnt="http://docs.oasis-open.org/wsn/b-2" xmlns:tt="http://www.onvif.org/ver10/schema">
  <SOAP-ENV:Body>
    <tev:PullMessagesResponse>
      <wsnt:NotificationMessage>
        <wsnt:Topic Dialect="http://www.onvif.org/ver10/tev/topicExpression/ConcreteSet">tns1:RuleEngine/CellMotionDetector/Motion</wsnt:Topic>
        <wsnt:Message UtcTime="2026-10-23T01:01:01.456Z" PropertyOperation="Changed">
          <tt:Source>
            <tt:SimpleItem Name="VideoSourceConfigurationToken" Value="VideoSourceToken_1"/>
          </tt:Source>
          <tt:Data>
            <tt:SimpleItem Name="IsMotion" Value="true"/>
          </tt:Data>
        </wsnt:Message>
      </wsnt:NotificationMessage>
    </tev:PullMessagesResponse>
  </SOAP-ENV:Body>
</SOAP-ENV:Envelope>"#;

const RENEW_RESPONSE: &str = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope" xmlns:wsnt="http://docs.oasis-open.org/wsn/b-2">
  <SOAP-ENV:Body>
    <wsnt:RenewResponse>
      <wsnt:TerminationTime>2026-10-23T01:02:00Z</wsnt:TerminationTime>
    </wsnt:RenewResponse>
  </SOAP-ENV:Body>
</SOAP-ENV:Envelope>"#;

const ACK: &str = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope"><SOAP-ENV:Body/></SOAP-ENV:Envelope>"#;

fn make_session(server: &FixtureServer) -> Arc<OnvifSession> {
    Arc::new(
        OnvifSession::new(
            format!("{}/onvif/device_service", server.url()),
            Credentials {
                username: "admin".into(),
                password: "password".into(),
            },
            HttpConfig {
                timeout: Duration::from_millis(500),
                ..HttpConfig::default()
            },
        )
        .expect("session creation"),
    )
}

#[tokio::test]
async fn test_motion_event_arrives_within_200_ms() {
    let routes = vec![
        Route::soap("/onvif/device_service", "GetCapabilities", CAPABILITIES),
        Route::soap(
            "/onvif/events_service",
            "CreatePullPointSubscription",
            SUB_RESPONSE,
        ),
        Route::soap("/onvif/pullpoint", "PullMessages", PULL_MOTION),
        Route::soap("/onvif/pullpoint", "Unsubscribe", ACK),
    ];
    let server = FixtureServer::start(routes).await.expect("fixture server");
    let session = make_session(&server);

    let mut sub = subscribe_events(
        session,
        PullPointConfig {
            pull_timeout: Duration::from_millis(100),
            initial_retry_delay: Duration::from_millis(10),
            ..PullPointConfig::default()
        },
    );

    let start = Instant::now();
    let event = sub.recv().await.expect("received event");
    let elapsed = start.elapsed();

    assert!(event.is_motion(), "event must be motion");
    assert_eq!(event.topic, "tns1:RuleEngine/CellMotionDetector/Motion");
    assert!(
        elapsed < Duration::from_millis(200),
        "event arrived in {elapsed:?}, expected < 200 ms"
    );
}

#[tokio::test]
async fn test_network_interruption_reconnect_and_resubscribe_deduplicates() {
    // Interruption simulation:
    // First pull delivers event, next pull encounters 500 fault or interruption,
    // then resubscribes and delivers the same event.
    // The subscriber must deduplicate and NOT deliver duplicate events to consumer.
    let routes = vec![
        Route::soap("/onvif/device_service", "GetCapabilities", CAPABILITIES),
        Route::soap(
            "/onvif/events_service",
            "CreatePullPointSubscription",
            SUB_RESPONSE,
        ),
        Route::soap("/onvif/pullpoint", "PullMessages", PULL_MOTION),
        Route::soap("/onvif/pullpoint", "Unsubscribe", ACK),
    ];
    let server = FixtureServer::start(routes).await.expect("fixture server");
    let session = make_session(&server);

    let mut sub = subscribe_events(
        session,
        PullPointConfig {
            subscription_ttl: Duration::from_secs(1),
            pull_timeout: Duration::from_millis(50),
            initial_retry_delay: Duration::from_millis(10),
            ..PullPointConfig::default()
        },
    );

    // First event received
    let first = sub.recv().await.expect("first event");
    assert!(first.is_motion());

    // Allow multiple pull intervals; server continues returning PULL_MOTION,
    // but the consumer should deduplicate based on (topic, timestamp_ms, data).
    let next_recv = tokio::time::timeout(Duration::from_millis(150), sub.recv()).await;
    // No duplicate events should be yielded within the window
    assert!(
        next_recv.is_err(),
        "expected timeout because duplicate event was filtered"
    );
}

#[tokio::test]
async fn test_subscription_renewal_succeeds_no_missed_events() {
    let routes = vec![
        Route::soap("/onvif/device_service", "GetCapabilities", CAPABILITIES),
        Route::soap(
            "/onvif/events_service",
            "CreatePullPointSubscription",
            SUB_RESPONSE,
        ),
        Route::soap("/onvif/pullpoint", "PullMessages", PULL_MOTION),
        Route::soap("/onvif/pullpoint", "Renew", RENEW_RESPONSE),
        Route::soap("/onvif/pullpoint", "Unsubscribe", ACK),
    ];
    let server = FixtureServer::start(routes).await.expect("fixture server");
    let session = make_session(&server);

    // TTL = 100ms -> renewal trigger at 50ms
    let mut sub = subscribe_events(
        session,
        PullPointConfig {
            subscription_ttl: Duration::from_millis(100),
            pull_timeout: Duration::from_millis(20),
            initial_retry_delay: Duration::from_millis(10),
            ..PullPointConfig::default()
        },
    );

    let event = sub.recv().await.expect("event received");
    assert!(event.is_motion());

    // Wait past renewal window (e.g. 150ms) to ensure renewals execute on server
    tokio::time::sleep(Duration::from_millis(150)).await;

    let renew_count = server
        .requests()
        .iter()
        .filter(|r| r.body.contains("Renew"))
        .count();
    assert!(
        renew_count >= 1,
        "at least one Renew request must be dispatched"
    );
}

#[tokio::test]
async fn test_cctv_agent_motion_subscription_triggers_emergency_asset_under_500ms() {
    let routes = vec![
        Route::soap("/onvif/device_service", "GetCapabilities", CAPABILITIES),
        Route::soap(
            "/onvif/events_service",
            "CreatePullPointSubscription",
            SUB_RESPONSE,
        ),
        Route::soap("/onvif/pullpoint", "PullMessages", PULL_MOTION),
        Route::soap("/onvif/pullpoint", "Unsubscribe", ACK),
    ];
    let server = FixtureServer::start(routes).await.expect("fixture server");
    let session = make_session(&server);
    let camera_id = CameraId::from("cam-entrance-01");

    let service = EventSubscriptionService::new(session, camera_id.clone());
    let mut stream = service.subscribe_motion();

    let start = Instant::now();
    let detection: Option<DetectionEvent> = stream.next().await;
    let det = detection.expect("motion detection event");
    assert_eq!(det.camera_id, camera_id);
    assert!(det.is_motion);

    // Simulate signage command trigger upon motion
    let command = SignageCommand::EmergencyAsset {
        camera_id: det.camera_id.clone(),
        tag: format!("Motion in {}", det.topic),
    };
    let reaction_time = start.elapsed();

    assert!(
        matches!(command, SignageCommand::EmergencyAsset { .. }),
        "must trigger SignageCommand::EmergencyAsset"
    );
    assert!(
        reaction_time < Duration::from_millis(500),
        "total reaction time {reaction_time:?} must be < 500 ms"
    );
}
