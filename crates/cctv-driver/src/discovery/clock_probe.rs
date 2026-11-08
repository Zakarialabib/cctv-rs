//! Unauthenticated ONVIF clock probe and clock-drift calculation.
//!
//! Calls unauthenticated SOAP `tds:GetSystemDateAndTime` on `http://<ip>:<port>/onvif/device_service`
//! and calculates `observed_clock_offset_ms = camera_utc_ms - host_utc_ms`.

use std::net::IpAddr;
use std::time::Duration;
use thiserror::Error;

use cctv_transport::http::{HttpClient, HttpConfig, Method};
use cctv_transport::xml;

/// Discovery errors.
#[derive(Debug, Error)]
pub enum DiscoveryError {
    /// An HTTP transport error occurred.
    #[error("http transport error: {0}")]
    Http(String),
    /// An unexpected HTTP status code was received.
    #[error("unexpected HTTP status code: {0}")]
    HttpStatus(u16),
    /// The response XML could not be parsed.
    #[error("xml parse error: {0}")]
    Parse(String),
    /// A SOAP fault was returned by the device.
    #[error("soap fault: {0}")]
    SoapFault(String),
    /// An I/O error occurred.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// ONVIF SOAP envelope namespace.
const SOAP_ENV_NS: &str = "http://www.w3.org/2003/05/soap-envelope";
/// ONVIF Device service namespace.
const DEVICE_NS: &str = "http://www.onvif.org/ver10/device/wsdl";
/// Content-Type header for ONVIF SOAP 1.2 requests.
const SOAP_CONTENT_TYPE: &str = "application/soap+xml";

/// Build an unauthenticated `GetSystemDateAndTime` SOAP 1.2 request envelope.
pub fn build_get_system_date_and_time() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><s:Envelope xmlns:s="{SOAP_ENV_NS}" xmlns:tds="{DEVICE_NS}"><s:Body><tds:GetSystemDateAndTime/></s:Body></s:Envelope>"#
    )
}

/// Convert UTC (year, month, day, hour, min, sec) into milliseconds since Unix epoch (1970-01-01T00:00:00Z).
pub fn utc_to_epoch_ms(year: i64, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> i64 {
    let days = days_from_civil(year, month, day);
    let total_secs = days * 86_400 + (hour as i64) * 3600 + (min as i64) * 60 + (sec as i64);
    total_secs * 1_000
}

/// Days since 1970-01-01 from (year, month, day). Howard Hinnant's algorithm.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 } as i64) + 2) / 5 + (d as i64) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Parse the `GetSystemDateAndTimeResponse` XML to extract camera UTC time in milliseconds.
pub fn parse_system_date_and_time(xml_text: &str) -> Result<i64, DiscoveryError> {
    if let Some(fault) = xml::soap_fault(xml_text) {
        return Err(DiscoveryError::SoapFault(fault));
    }

    // Look for UTCDateTime element, or DateTime if UTCDateTime is omitted.
    let dt_body = xml::element_body(xml_text, "UTCDateTime")
        .or_else(|| xml::element_body(xml_text, "LocalDateTime"))
        .or_else(|| xml::element_body(xml_text, "GetSystemDateAndTimeResponse"))
        .unwrap_or(xml_text);

    let time_body = xml::element_body(dt_body, "Time").unwrap_or(dt_body);
    let date_body = xml::element_body(dt_body, "Date").unwrap_or(dt_body);

    let hour: u32 = xml::element_text(time_body, "Hour")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| DiscoveryError::Parse("missing or invalid Hour".into()))?;

    let min: u32 = xml::element_text(time_body, "Minute")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| DiscoveryError::Parse("missing or invalid Minute".into()))?;

    let sec: u32 = xml::element_text(time_body, "Second")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| DiscoveryError::Parse("missing or invalid Second".into()))?;

    let year: i64 = xml::element_text(date_body, "Year")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| DiscoveryError::Parse("missing or invalid Year".into()))?;

    let month: u32 = xml::element_text(date_body, "Month")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| DiscoveryError::Parse("missing or invalid Month".into()))?;

    let day: u32 = xml::element_text(date_body, "Day")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| DiscoveryError::Parse("missing or invalid Day".into()))?;

    Ok(utc_to_epoch_ms(year, month, day, hour, min, sec))
}

/// Probe an ONVIF camera's clock unauthenticated and compute the clock offset in milliseconds.
///
/// Sends `tds:GetSystemDateAndTime` to `http://<ip>:<port>/onvif/device_service`.
/// Returns `observed_clock_offset_ms = camera_utc_ms - host_utc_ms`.
pub async fn probe_onvif_clock(
    ip: IpAddr,
    port: u16,
    host_utc_ms: i64,
) -> Result<i64, DiscoveryError> {
    let url = match ip {
        IpAddr::V4(v4) => format!("http://{v4}:{port}/onvif/device_service"),
        IpAddr::V6(v6) => format!("http://[{v6}]:{port}/onvif/device_service"),
    };

    let client = HttpClient::new(HttpConfig {
        timeout: Duration::from_millis(1_000),
        accept_invalid_certs: true,
        username: None,
        password: None,
    })
    .map_err(|e| DiscoveryError::Http(e.to_string()))?;

    let req_body = build_get_system_date_and_time();
    let resp = client
        .request(
            Method::Post,
            &url,
            Some(SOAP_CONTENT_TYPE),
            Some(req_body.into_bytes()),
        )
        .await
        .map_err(|e| DiscoveryError::Http(e.to_string()))?;

    if !resp.is_success() {
        return Err(DiscoveryError::HttpStatus(resp.status));
    }

    let text = resp.text();
    let camera_utc_ms = parse_system_date_and_time(&text)?;
    let observed_clock_offset_ms = camera_utc_ms - host_utc_ms;

    Ok(observed_clock_offset_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_REPLY: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
    <s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:tt="http://www.onvif.org/ver10/schema" xmlns:tds="http://www.onvif.org/ver10/device/wsdl">
      <s:Body>
        <tds:GetSystemDateAndTimeResponse>
          <tds:SystemDateAndTime>
            <tt:DateTimeType>Manual</tt:DateTimeType>
            <tt:DaylightSavings>false</tt:DaylightSavings>
            <tt:TimeZone><tt:TZ>UTC0</tt:TZ></tt:TimeZone>
            <tt:UTCDateTime>
              <tt:Time><tt:Hour>14</tt:Hour><tt:Minute>30</tt:Minute><tt:Second>15</tt:Second></tt:Time>
              <tt:Date><tt:Year>2026</tt:Year><tt:Month>11</tt:Month><tt:Day>1</tt:Day></tt:Date>
            </tt:UTCDateTime>
          </tds:SystemDateAndTime>
        </tds:GetSystemDateAndTimeResponse>
      </s:Body>
    </s:Envelope>"#;

    #[test]
    fn parse_date_and_time_computes_utc_ms() {
        let camera_ms = parse_system_date_and_time(SAMPLE_REPLY).expect("parse failed");
        assert_eq!(utc_to_epoch_ms(2026, 11, 1, 14, 30, 15), camera_ms);

        let host_utc_ms = camera_ms - 5_000;
        let offset = camera_ms - host_utc_ms;
        assert_eq!(offset, 5_000);
    }

    #[test]
    fn epoch_conversion_known_values() {
        assert_eq!(utc_to_epoch_ms(1970, 1, 1, 0, 0, 0), 0);
        assert_eq!(utc_to_epoch_ms(2000, 2, 29, 0, 0, 0), 951_782_400_000);
    }
}
