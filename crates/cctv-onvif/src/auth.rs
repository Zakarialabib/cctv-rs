//! Skewed-clock WS-Security `UsernameToken` authentication for ONVIF.
//!
//! Cameras frequently suffer from clock drift or are configured with incorrect
//! timezones or NTP settings. WS-Security UsernameToken `#PasswordDigest` relies
//! on `<wsu:Created>` and `SHA1(nonce + created + password)` matching what the
//! device expects.
//!
//! This module computes `<wsu:Created>` and the password digest taking into
//! account `host_utc_ms + observed_clock_offset_ms`.

use base64::Engine as _;
use cctv_core::DecryptedCameraAuth;
use sha1::{Digest as _, Sha1};

const WSSE_NS: &str =
    "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd";
const WSU_NS: &str =
    "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd";
const PASSWORD_DIGEST: &str =
    "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-username-token-profile-1.0#PasswordDigest";
const NONCE_B64: &str =
    "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-soap-message-security-1.0#Base64Binary";

/// Calculate the effective UTC timestamp in milliseconds taking into account
/// the observed camera clock offset.
#[inline]
pub fn effective_utc_ms(host_utc_ms: i64, observed_clock_offset_ms: i64) -> i64 {
    host_utc_ms.saturating_add(observed_clock_offset_ms)
}

/// Format a UTC millisecond timestamp as ISO-8601 UTC (`YYYY-MM-DDTHH:MM:SS.sssZ`).
pub fn iso8601_utc_ms(utc_ms: i64) -> String {
    let non_neg_ms = if utc_ms < 0 { 0 } else { utc_ms as u64 };
    let secs = non_neg_ms / 1000;
    let millis = non_neg_ms % 1000;

    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(i64::try_from(days).unwrap_or(0));
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
        millis
    )
}

/// Days since 1970-01-01 -> (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Compute `Base64(SHA1(nonce || created || password))`.
pub fn compute_password_digest(nonce: &[u8], created: &str, password: &str) -> String {
    let mut h = Sha1::new();
    h.update(nonce);
    h.update(created.as_bytes());
    h.update(password.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(h.finalize())
}

/// Generate the `<wsse:Security>` SOAP header for skewed-clock WS-Security.
pub fn build_ws_security_header(
    username: &str,
    auth: &DecryptedCameraAuth,
    nonce_bytes: &[u8],
    host_utc_ms: i64,
    observed_clock_offset_ms: i64,
) -> String {
    let effective_ms = effective_utc_ms(host_utc_ms, observed_clock_offset_ms);
    let created = iso8601_utc_ms(effective_ms);
    let digest = compute_password_digest(nonce_bytes, &created, auth.password.as_str());
    let nonce_b64 = base64::engine::general_purpose::STANDARD.encode(nonce_bytes);
    let escaped_user = cctv_transport::xml::escape(username);

    format!(
        r#"<wsse:Security xmlns:wsse="{WSSE_NS}" xmlns:wsu="{WSU_NS}" s:mustUnderstand="1"><wsse:UsernameToken><wsse:Username>{escaped_user}</wsse:Username><wsse:Password Type="{PASSWORD_DIGEST}">{digest}</wsse:Password><wsse:Nonce EncodingType="{NONCE_B64}">{nonce_b64}</wsse:Nonce><wsu:Created>{created}</wsu:Created></wsse:UsernameToken></wsse:Security>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_offset_skew_calculation() {
        let host = 1_700_000_000_000i64;
        let offset_pos = 5_000i64;
        assert_eq!(effective_utc_ms(host, offset_pos), 1_700_000_005_000i64);

        let offset_neg = -10_000i64;
        assert_eq!(effective_utc_ms(host, offset_neg), 1_699_999_990_000i64);
    }

    #[test]
    fn iso8601_utc_ms_formats_with_milliseconds() {
        // 0 -> 1970-01-01T00:00:00.000Z
        assert_eq!(iso8601_utc_ms(0), "1970-01-01T00:00:00.000Z");
        // 123 ms
        assert_eq!(iso8601_utc_ms(123), "1970-01-01T00:00:00.123Z");
        // 2026-10-23T01:01:01.456Z
        let ts = (1_792_713_600 + 3_661) * 1000 + 456;
        assert_eq!(iso8601_utc_ms(ts), "2026-10-23T01:01:01.456Z");
    }

    #[test]
    fn ws_security_header_contains_skewed_created_and_digest() {
        let auth = DecryptedCameraAuth::new("secret_pass", "");
        let nonce = b"0123456789abcdef";
        let host_utc_ms = 1_792_713_600_000i64; // 2026-10-23T00:00:00.000Z
        let offset_ms = 3_661_500i64; // + 1 hr 1 min 1 sec 500 ms -> 2026-10-23T01:01:01.500Z

        let header = build_ws_security_header("admin&user", &auth, nonce, host_utc_ms, offset_ms);
        assert!(header.contains("<wsse:Username>admin&amp;user</wsse:Username>"));
        assert!(header.contains("<wsu:Created>2026-10-23T01:01:01.500Z</wsu:Created>"));
        assert!(header.contains("MDEyMzQ1Njc4OWFiY2RlZg=="));

        let expected_created = "2026-10-23T01:01:01.500Z";
        let expected_digest = compute_password_digest(nonce, expected_created, "secret_pass");
        assert!(header.contains(&format!(
            r#"<wsse:Password Type="{PASSWORD_DIGEST}">{expected_digest}</wsse:Password>"#
        )));
        assert!(!header.contains("secret_pass"));
    }
}
