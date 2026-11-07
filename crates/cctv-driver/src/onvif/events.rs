//! ONVIF Event Service and WS-Eventing / WS-BaseNotification client.
//!
//! Provides SOAP builders and XML response parsers for WS-Eventing and ONVIF Pull-Point
//! event notifications:
//! - `CreatePullPointSubscription`
//! - `PullMessages`
//! - `Renew`
//! - `Unsubscribe`
//!
//! Topics and filters follow the ONVIF Core Specification (e.g. `tns1:RuleEngine/CellMotionDetector/Motion`,
//! `tns1:RuleEngine/Motion`, `tns1:Device/Trigger`).

use super::error::OnvifError;
use super::session::OnvifSession;
use cctv_transport::xml;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{sleep, Instant};

/// ONVIF Events service namespace (`tev:`).
pub const EVENTS_NS: &str = "http://www.onvif.org/ver10/events/wsdl";
/// WS-BaseNotification namespace (`wsnt:`).
pub const WSNT_NS: &str = "http://docs.oasis-open.org/wsn/b-2";
/// WS-Addressing namespace (`wsa:`).
pub const WSA_NS: &str = "http://www.w3.org/2005/08/addressing";
/// WS-Eventing namespace (`wse:`).
pub const WSE_NS: &str = "http://schemas.xmlsoap.org/ws/2004/08/eventing";
/// ONVIF Topic namespace (`tns1:`).
pub const TOPIC_NS: &str = "http://www.onvif.org/ver10/topics/topicns";

/// Format a `Duration` as an ISO 8601 duration string, e.g. `PT60S`.
pub fn format_iso_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    format!("PT{secs}S")
}

/// Parse an ISO 8601 duration (e.g. `PT60S`, `PT1M`, `PT1H30M`, `PT300.000S`) to `Duration`.
/// Returns None if invalid or empty.
pub fn parse_iso_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    if !s.starts_with('P') {
        return None;
    }
    let rest = &s[1..];
    let (date_part, time_part) = match rest.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (rest, None),
    };

    let mut total_secs: u64 = 0;
    let mut num_buf = String::new();

    for ch in date_part.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            num_buf.push(ch);
        } else {
            let val: f64 = num_buf.parse().ok()?;
            num_buf.clear();
            match ch {
                'Y' => total_secs = total_secs.saturating_add((val * 31_536_000.0) as u64),
                'M' => total_secs = total_secs.saturating_add((val * 2_592_000.0) as u64),
                'D' => total_secs = total_secs.saturating_add((val * 86_400.0) as u64),
                _ => {}
            }
        }
    }

    if let Some(time) = time_part {
        num_buf.clear();
        for ch in time.chars() {
            if ch.is_ascii_digit() || ch == '.' {
                num_buf.push(ch);
            } else {
                let val: f64 = num_buf.parse().ok()?;
                num_buf.clear();
                match ch {
                    'H' => total_secs = total_secs.saturating_add((val * 3600.0) as u64),
                    'M' => total_secs = total_secs.saturating_add((val * 60.0) as u64),
                    'S' => total_secs = total_secs.saturating_add(val.round() as u64),
                    _ => {}
                }
            }
        }
    }

    Some(Duration::from_secs(total_secs))
}

/// Parse ISO 8601 timestamp string (`YYYY-MM-DDTHH:MM:SSZ` or with millis) into Unix millis.
pub fn parse_iso_timestamp_ms(s: &str) -> Option<u64> {
    let s = s.trim();
    // Example: 2026-10-23T01:01:01Z or 2026-10-23T01:01:01.123Z
    let (date_part, time_part) = s.split_once('T')?;
    let time_clean = time_part.trim_end_matches('Z');
    let mut date_toks = date_part.split('-');
    let year: i32 = date_toks.next()?.parse().ok()?;
    let month: u32 = date_toks.next()?.parse().ok()?;
    let day: u32 = date_toks.next()?.parse().ok()?;

    let (hms_part, millis) = if let Some((hms, ms_str)) = time_clean.split_once('.') {
        let ms: u64 = ms_str.get(..3).unwrap_or(ms_str).parse().ok().unwrap_or(0);
        let padded_ms = match ms_str.len() {
            1 => ms * 100,
            2 => ms * 10,
            _ => ms,
        };
        (hms, padded_ms)
    } else {
        (time_clean, 0)
    };

    let mut time_toks = hms_part.split(':');
    let hour: u64 = time_toks.next()?.parse().ok()?;
    let min: u64 = time_toks.next()?.parse().ok()?;
    let sec: u64 = time_toks.next()?.parse().ok()?;

    let days = days_from_civil(year, month, day);
    let total_secs = (days as u64)
        .checked_mul(86_400)?
        .checked_add(hour * 3600 + min * 60 + sec)?;

    Some(total_secs * 1000 + millis)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = year - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let m = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era as i64) * 146097 + (doe as i64) - 719468
}

/// Build `CreatePullPointSubscription` SOAP request body.
///
/// Can specify initial termination duration (e.g. `PT60S`) and optional topic filter.
pub fn build_create_pull_point_subscription(
    initial_termination_time: Option<Duration>,
    topic_filter: Option<&str>,
) -> String {
    let termination = initial_termination_time
        .map(|d| {
            format!(
                "<tev:InitialTerminationTime>{}</tev:InitialTerminationTime>",
                format_iso_duration(d)
            )
        })
        .unwrap_or_default();

    let filter = topic_filter
        .map(|t| {
            format!(
                r#"<tev:Filter><wsnt:TopicExpression Dialect="http://www.onvif.org/ver10/tev/topicExpression/ConcreteSet" xmlns:tns1="{TOPIC_NS}">{}</wsnt:TopicExpression></tev:Filter>"#,
                cctv_transport::xml::escape(t)
            )
        })
        .unwrap_or_default();

    format!(
        r#"<tev:CreatePullPointSubscription xmlns:tev="{EVENTS_NS}" xmlns:wsnt="{WSNT_NS}">{filter}{termination}</tev:CreatePullPointSubscription>"#
    )
}

/// Build `PullMessages` SOAP request body.
///
/// Requests messages from the subscription with a maximum timeout and message limit.
pub fn build_pull_messages(timeout: Duration, message_limit: u32) -> String {
    format!(
        r#"<tev:PullMessages xmlns:tev="{EVENTS_NS}"><tev:Timeout>{}</tev:Timeout><tev:MessageLimit>{}</tev:MessageLimit></tev:PullMessages>"#,
        format_iso_duration(timeout),
        message_limit
    )
}

/// Build `Renew` SOAP request body.
///
/// Renews a subscription for the specified duration.
pub fn build_renew(termination_time: Duration) -> String {
    format!(
        r#"<wsnt:Renew xmlns:wsnt="{WSNT_NS}"><wsnt:TerminationTime>{}</wsnt:TerminationTime></wsnt:Renew>"#,
        format_iso_duration(termination_time)
    )
}

/// Build `Unsubscribe` SOAP request body.
pub fn build_unsubscribe() -> &'static str {
    r#"<wsnt:Unsubscribe xmlns:wsnt="{WSNT_NS}"/>"#
}

/// Pull point subscription information from `CreatePullPointSubscriptionResponse`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionReference {
    /// Subscription address (pull-point URL).
    pub address: String,
    /// Current time reported by camera, if any.
    pub current_time: Option<String>,
    /// Termination time of subscription.
    pub termination_time: Option<String>,
}

/// Parse `CreatePullPointSubscriptionResponse`.
pub fn parse_create_pull_point_subscription(
    body: &str,
) -> Result<SubscriptionReference, OnvifError> {
    let scope = xml::element_body(body, "CreatePullPointSubscriptionResponse")
        .or_else(|| xml::element_body(body, "SubscribeResponse"))
        .ok_or_else(|| {
            OnvifError::Parse("no CreatePullPointSubscriptionResponse in reply".into())
        })?;

    let sub_ref = xml::element_body(scope, "SubscriptionReference")
        .ok_or_else(|| OnvifError::Parse("no SubscriptionReference in response".into()))?;

    let address = xml::element_text(sub_ref, "Address")
        .ok_or_else(|| OnvifError::Parse("no Address in SubscriptionReference".into()))?;

    let current_time = xml::element_text(scope, "CurrentTime");
    let termination_time = xml::element_text(scope, "TerminationTime");

    Ok(SubscriptionReference {
        address,
        current_time,
        termination_time,
    })
}

/// Renew response information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenewResponse {
    /// Current camera time.
    pub current_time: Option<String>,
    /// New termination time.
    pub termination_time: Option<String>,
}

/// Parse `RenewResponse`.
pub fn parse_renew(body: &str) -> Result<RenewResponse, OnvifError> {
    let scope = xml::element_body(body, "RenewResponse")
        .ok_or_else(|| OnvifError::Parse("no RenewResponse in reply".into()))?;

    Ok(RenewResponse {
        current_time: xml::element_text(scope, "CurrentTime"),
        termination_time: xml::element_text(scope, "TerminationTime"),
    })
}

/// One ONVIF Notification Event parsed from a `PullMessagesResponse`.
#[derive(Debug, Clone, PartialEq)]
pub struct OnvifEvent {
    /// Topic identifier (e.g. `tns1:RuleEngine/CellMotionDetector/Motion`).
    pub topic: String,
    /// UTC timestamp in milliseconds when the event was generated.
    pub timestamp_ms: u64,
    /// Property operation: e.g. "Changed", "Initialized", "Deleted".
    pub property_operation: Option<String>,
    /// Key-value data attributes reported in the message (e.g. `State: true`, `IsMotion: true`).
    pub data: Vec<(String, String)>,
}

impl OnvifEvent {
    /// Returns true if this event indicates active motion.
    pub fn is_motion(&self) -> bool {
        let topic_lower = self.topic.to_lowercase();
        let is_motion_topic = topic_lower.contains("motion");
        if !is_motion_topic {
            return false;
        }

        if let Some(op) = &self.property_operation {
            if op.eq_ignore_ascii_case("deleted") {
                return false;
            }
        }

        let mut has_state_item = false;
        // Check data values
        for (key, val) in &self.data {
            let k = key.to_lowercase();
            let v = val.trim().to_lowercase();
            if k == "state" || k == "ismotion" || k == "active" || k == "data" {
                has_state_item = true;
                if v == "false" || v == "0" || v == "inactive" {
                    return false;
                }
                if v == "true" || v == "1" || v == "active" {
                    return true;
                }
            }
        }

        // If a state item was present but not active, return false; otherwise default to true for motion topic
        !has_state_item
    }
}

/// Parse `PullMessagesResponse` extracting all notification messages.
pub fn parse_pull_messages(body: &str) -> Result<Vec<OnvifEvent>, OnvifError> {
    let scope = xml::element_body(body, "PullMessagesResponse")
        .ok_or_else(|| OnvifError::Parse("no PullMessagesResponse in reply".into()))?;

    let mut events = Vec::new();
    let mut cursor = scope;

    while let Some(msg_idx) = cursor
        .find("<NotificationMessage")
        .or_else(|| cursor.find("<wsnt:NotificationMessage"))
    {
        let after_start = &cursor[msg_idx..];
        let end_tag = if after_start.contains("</wsnt:NotificationMessage>") {
            "</wsnt:NotificationMessage>"
        } else {
            "</NotificationMessage>"
        };

        if let Some(end_offset) = after_start.find(end_tag) {
            let end_idx = end_offset + end_tag.len();
            let msg_xml = &after_start[..end_idx];
            cursor = &after_start[end_idx..];

            if let Some(event) = parse_notification_message(msg_xml) {
                events.push(event);
            }
        } else {
            break;
        }
    }

    Ok(events)
}

fn parse_notification_message(xml_snippet: &str) -> Option<OnvifEvent> {
    let topic = xml::element_text(xml_snippet, "Topic").unwrap_or_else(|| "Unknown".to_string());

    let message_elements = xml::elements(xml_snippet, "Message");
    let message_elem = message_elements.first()?;
    let timestamp_str = message_elem.attr("UtcTime");

    let timestamp_ms = timestamp_str
        .as_deref()
        .and_then(parse_iso_timestamp_ms)
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0)
        });

    let property_operation = message_elem.attr("PropertyOperation");

    let mut data = Vec::new();
    if let Some(data_body) = xml::element_body(message_elem.body, "Data") {
        for item in xml::elements(data_body, "SimpleItem") {
            if let (Some(name), Some(val)) = (item.attr("Name"), item.attr("Value")) {
                data.push((name, val));
            }
        }
    }

    Some(OnvifEvent {
        topic,
        timestamp_ms,
        property_operation,
        data,
    })
}

/// Configuration for pull-point subscription stream.
#[derive(Debug, Clone)]
pub struct PullPointConfig {
    /// Topic filter, e.g. `tns1:RuleEngine/Motion` or `tns1:RuleEngine//.`.
    pub topic_filter: Option<String>,
    /// Subscription duration on camera before expiry.
    pub subscription_ttl: Duration,
    /// Timeout for each PullMessages request.
    pub pull_timeout: Duration,
    /// Message limit per PullMessages request.
    pub message_limit: u32,
    /// Capacity of downstream buffer channel for backpressure.
    pub channel_capacity: usize,
    /// Initial retry delay for backoff.
    pub initial_retry_delay: Duration,
    /// Maximum retry delay for backoff.
    pub max_retry_delay: Duration,
}

impl Default for PullPointConfig {
    fn default() -> Self {
        Self {
            topic_filter: None,
            subscription_ttl: Duration::from_secs(60),
            pull_timeout: Duration::from_secs(5),
            message_limit: 10,
            channel_capacity: 100,
            initial_retry_delay: Duration::from_millis(50),
            max_retry_delay: Duration::from_secs(5),
        }
    }
}

/// Active Pull-Point subscription handle.
pub struct EventSubscription {
    receiver: mpsc::Receiver<OnvifEvent>,
    shutdown: Arc<AtomicBool>,
}

impl EventSubscription {
    /// Receive next event from stream. Returns None when stream ends.
    pub async fn recv(&mut self) -> Option<OnvifEvent> {
        self.receiver.recv().await
    }

    /// Stop the subscription task and release resources.
    pub fn close(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    /// Clone handle sharing the shutdown flag with an empty receiver channel.
    pub fn clone_handle(&self) -> Self {
        let (_tx, rx) = mpsc::channel(1);
        Self {
            receiver: rx,
            shutdown: Arc::clone(&self.shutdown),
        }
    }
}

impl Drop for EventSubscription {
    fn drop(&mut self) {
        self.close();
    }
}

/// Start an asynchronous pull-point subscription event stream against an `OnvifSession`.
///
/// Features:
/// - Subscribes via `CreatePullPointSubscription`
/// - Periodically renews before expiry (`subscription_ttl / 2`)
/// - Exponential backoff on transient network failures
/// - Reconnects and resubscribes automatically
/// - Deduplicates events across reconnects using (topic, timestamp_ms, data)
/// - Backpressure: if downstream channel is full, pauses renewal so edge camera subscription can expire,
///   then resumes when consumer catches up.
pub fn subscribe_events(session: Arc<OnvifSession>, config: PullPointConfig) -> EventSubscription {
    let (tx, rx) = mpsc::channel(config.channel_capacity);
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown);

    tokio::spawn(async move {
        run_subscription_loop(session, config, tx, shutdown_clone).await;
    });

    EventSubscription {
        receiver: rx,
        shutdown,
    }
}

async fn run_subscription_loop(
    session: Arc<OnvifSession>,
    config: PullPointConfig,
    tx: mpsc::Sender<OnvifEvent>,
    shutdown: Arc<AtomicBool>,
) {
    type EventKey = (String, u64, Vec<(String, String)>);
    let mut retry_delay = config.initial_retry_delay;
    let mut seen_events: HashSet<EventKey> = HashSet::new();

    while !shutdown.load(Ordering::SeqCst) {
        // Step 1: Create subscription
        let sub = match session
            .create_pull_point_subscription(
                Some(config.subscription_ttl),
                config.topic_filter.as_deref(),
            )
            .await
        {
            Ok(s) => {
                retry_delay = config.initial_retry_delay;
                s
            }
            Err(_) => {
                if shutdown.load(Ordering::SeqCst) {
                    break;
                }
                sleep(retry_delay).await;
                retry_delay = (retry_delay * 2).min(config.max_retry_delay);
                continue;
            }
        };

        let pull_url = sub.address.clone();
        let mut last_renewal = Instant::now();
        let renewal_interval = config.subscription_ttl / 2;

        // Step 2: Event pulling and renewal loop
        while !shutdown.load(Ordering::SeqCst) {
            // Check backpressure: if receiver buffer has no remaining capacity,
            // we intentionally DO NOT renew and let the subscription expire,
            // yielding CPU and sleeping briefly before checking again.
            if tx.capacity() == 0 {
                sleep(Duration::from_millis(100)).await;
                if last_renewal.elapsed() >= config.subscription_ttl {
                    // Subscription expired on camera due to sustained backpressure;
                    // break to outer loop to resubscribe once consumer drains.
                    break;
                }
                continue;
            }

            // Check if renewal is due
            if last_renewal.elapsed() >= renewal_interval {
                match session
                    .renew_subscription(&pull_url, config.subscription_ttl)
                    .await
                {
                    Ok(_) => {
                        last_renewal = Instant::now();
                    }
                    Err(_) => {
                        // Renewal failed: if expired or failing, break to re-subscribe
                        if last_renewal.elapsed() >= config.subscription_ttl {
                            break;
                        }
                    }
                }
            }

            // Pull messages
            match session
                .pull_messages(&pull_url, config.pull_timeout, config.message_limit)
                .await
            {
                Ok(events) => {
                    retry_delay = config.initial_retry_delay;
                    for event in events {
                        let dedup_key =
                            (event.topic.clone(), event.timestamp_ms, event.data.clone());
                        if seen_events.contains(&dedup_key) {
                            continue;
                        }
                        seen_events.insert(dedup_key);
                        // Bounded set of seen events
                        if seen_events.len() > 10_000 {
                            seen_events.clear();
                        }

                        if tx.send(event).await.is_err() {
                            // Receiver dropped
                            let _ = session.unsubscribe(&pull_url).await;
                            return;
                        }
                    }
                }
                Err(_) => {
                    // Pull error or connection lost: check if subscription expired
                    if last_renewal.elapsed() >= config.subscription_ttl {
                        break;
                    }
                    sleep(retry_delay).await;
                    retry_delay = (retry_delay * 2).min(config.max_retry_delay);
                }
            }
        }

        // Clean up subscription if possible before reconnecting
        let _ = session.unsubscribe(&pull_url).await;
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        sleep(retry_delay).await;
        retry_delay = (retry_delay * 2).min(config.max_retry_delay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_duration_formatting_and_parsing() {
        assert_eq!(format_iso_duration(Duration::from_secs(60)), "PT60S");
        assert_eq!(parse_iso_duration("PT60S"), Some(Duration::from_secs(60)));
        assert_eq!(parse_iso_duration("PT1M"), Some(Duration::from_secs(60)));
        assert_eq!(
            parse_iso_duration("PT1H30M"),
            Some(Duration::from_secs(5400))
        );
        assert_eq!(
            parse_iso_duration("P0Y0M0DT0H0M10S"),
            Some(Duration::from_secs(10))
        );
    }

    #[test]
    fn iso_timestamp_parsing() {
        let ts = parse_iso_timestamp_ms("1970-01-01T00:00:00Z").unwrap();
        assert_eq!(ts, 0);

        let ts_ms = parse_iso_timestamp_ms("1970-01-01T00:00:00.123Z").unwrap();
        assert_eq!(ts_ms, 123);

        let ts2 = parse_iso_timestamp_ms("2026-10-23T01:01:01.456Z").unwrap();
        assert_eq!(ts2, 1_792_717_261_456);
    }

    #[test]
    fn build_and_parse_create_pull_point_subscription() {
        let req = build_create_pull_point_subscription(
            Some(Duration::from_secs(60)),
            Some("tns1:RuleEngine/Motion"),
        );
        assert!(req.contains("<tev:InitialTerminationTime>PT60S</tev:InitialTerminationTime>"));
        assert!(req.contains("tns1:RuleEngine/Motion"));

        let reply = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope" xmlns:tev="http://www.onvif.org/ver10/events/wsdl" xmlns:wsa="http://www.w3.org/2005/08/addressing">
          <SOAP-ENV:Body>
            <tev:CreatePullPointSubscriptionResponse>
              <tev:SubscriptionReference>
                <wsa:Address>http://192.168.1.100/onvif/events/pullpoint?sub=42</wsa:Address>
              </tev:SubscriptionReference>
              <wsnt:CurrentTime xmlns:wsnt="http://docs.oasis-open.org/wsn/b-2">2026-10-23T01:00:00Z</wsnt:CurrentTime>
              <wsnt:TerminationTime xmlns:wsnt="http://docs.oasis-open.org/wsn/b-2">2026-10-23T01:01:00Z</wsnt:TerminationTime>
            </tev:CreatePullPointSubscriptionResponse>
          </SOAP-ENV:Body>
        </SOAP-ENV:Envelope>"#;

        let parsed = parse_create_pull_point_subscription(reply).unwrap();
        assert_eq!(
            parsed.address,
            "http://192.168.1.100/onvif/events/pullpoint?sub=42"
        );
        assert_eq!(parsed.current_time.as_deref(), Some("2026-10-23T01:00:00Z"));
        assert_eq!(
            parsed.termination_time.as_deref(),
            Some("2026-10-23T01:01:00Z")
        );
    }

    #[test]
    fn build_and_parse_renew() {
        let req = build_renew(Duration::from_secs(120));
        assert!(req.contains("<wsnt:TerminationTime>PT120S</wsnt:TerminationTime>"));

        let reply = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope" xmlns:wsnt="http://docs.oasis-open.org/wsn/b-2">
          <SOAP-ENV:Body>
            <wsnt:RenewResponse>
              <wsnt:TerminationTime>2026-10-23T01:03:00Z</wsnt:TerminationTime>
              <wsnt:CurrentTime>2026-10-23T01:01:00Z</wsnt:CurrentTime>
            </wsnt:RenewResponse>
          </SOAP-ENV:Body>
        </SOAP-ENV:Envelope>"#;

        let parsed = parse_renew(reply).unwrap();
        assert_eq!(
            parsed.termination_time.as_deref(),
            Some("2026-10-23T01:03:00Z")
        );
    }

    #[test]
    fn build_and_parse_pull_messages() {
        let req = build_pull_messages(Duration::from_secs(5), 10);
        assert!(req.contains("<tev:Timeout>PT5S</tev:Timeout>"));
        assert!(req.contains("<tev:MessageLimit>10</tev:MessageLimit>"));

        let reply = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope" xmlns:tev="http://www.onvif.org/ver10/events/wsdl" xmlns:wsnt="http://docs.oasis-open.org/wsn/b-2" xmlns:tt="http://www.onvif.org/ver10/schema">
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
                    <tt:SimpleItem Name="State" Value="true"/>
                  </tt:Data>
                </wsnt:Message>
              </wsnt:NotificationMessage>
            </tev:PullMessagesResponse>
          </SOAP-ENV:Body>
        </SOAP-ENV:Envelope>"#;

        let events = parse_pull_messages(reply).unwrap();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.topic, "tns1:RuleEngine/CellMotionDetector/Motion");
        assert_eq!(event.timestamp_ms, 1_792_717_261_456);
        assert_eq!(event.property_operation.as_deref(), Some("Changed"));
        assert!(event.is_motion());
        assert_eq!(
            event.data,
            vec![
                ("IsMotion".to_string(), "true".to_string()),
                ("State".to_string(), "true".to_string())
            ]
        );
    }
}
