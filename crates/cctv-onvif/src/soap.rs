//! SOAP 1.2 envelopes and the WS-Security `UsernameToken` header.
//!
//! ONVIF authenticates at the message level: each request carries a
//! `UsernameToken` whose password is a digest,
//! `Base64(SHA1(nonce ‖ created ‖ password))` (WS-Security UsernameToken
//! Profile 1.0, `#PasswordDigest`).  The digest is a pure function of its
//! inputs so it is tested with fixed values; only [`Credentials::header`]
//! draws a fresh nonce and the current time.

use base64::Engine as _;
use sha1::{Digest as _, Sha1};

/// SOAP 1.2 envelope namespace.
pub const SOAP_ENV_NS: &str = "http://www.w3.org/2003/05/soap-envelope";
/// ONVIF schema namespace (`tt:`).
pub const SCHEMA_NS: &str = "http://www.onvif.org/ver10/schema";
/// Device management service namespace (`tds:`).
pub const DEVICE_NS: &str = "http://www.onvif.org/ver10/device/wsdl";
/// Media service namespace (`trt:`).
pub const MEDIA_NS: &str = "http://www.onvif.org/ver10/media/wsdl";
/// PTZ service namespace (`tptz:`).
pub const PTZ_NS: &str = "http://www.onvif.org/ver20/ptz/wsdl";

const WSSE_NS: &str =
    "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-secext-1.0.xsd";
const WSU_NS: &str =
    "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-wssecurity-utility-1.0.xsd";
const PASSWORD_DIGEST: &str = "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-username-token-profile-1.0#PasswordDigest";
const NONCE_B64: &str = "http://docs.oasis-open.org/wss/2004/01/oasis-200401-wss-soap-message-security-1.0#Base64Binary";

/// ONVIF credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    /// Username.
    pub username: String,
    /// Password.
    pub password: String,
}

impl Credentials {
    /// A `<wsse:Security>` header with a fresh nonce and the current UTC time.
    pub fn header(&self) -> String {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        security_header(
            &self.username,
            &self.password,
            &fresh_nonce(),
            &iso8601_utc(secs),
        )
    }
}

/// `Base64(SHA1(nonce ‖ created ‖ password))`.
pub fn password_digest(nonce: &[u8], created: &str, password: &str) -> String {
    let mut h = Sha1::new();
    h.update(nonce);
    h.update(created.as_bytes());
    h.update(password.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(h.finalize())
}

/// Build the `<wsse:Security>` header for fixed inputs.
pub fn security_header(username: &str, password: &str, nonce: &[u8], created: &str) -> String {
    let digest = password_digest(nonce, created, password);
    let nonce_b64 = base64::engine::general_purpose::STANDARD.encode(nonce);
    format!(
        r#"<wsse:Security xmlns:wsse="{WSSE_NS}" xmlns:wsu="{WSU_NS}" s:mustUnderstand="1"><wsse:UsernameToken><wsse:Username>{user}</wsse:Username><wsse:Password Type="{PASSWORD_DIGEST}">{digest}</wsse:Password><wsse:Nonce EncodingType="{NONCE_B64}">{nonce_b64}</wsse:Nonce><wsu:Created>{created}</wsu:Created></wsse:UsernameToken></wsse:Security>"#,
        user = cctv_transport::xml::escape(username),
    )
}

/// Wrap a body in a SOAP 1.2 envelope, with an optional header.
///
/// The `tt`, `tds`, `trt` and `tptz` prefixes are declared on the envelope, so
/// body fragments may use them freely.
pub fn envelope(header: Option<&str>, body: &str) -> String {
    let header = header
        .map(|h| format!("<s:Header>{h}</s:Header>"))
        .unwrap_or_default();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="{SOAP_ENV_NS}" xmlns:tt="{SCHEMA_NS}" xmlns:tds="{DEVICE_NS}" xmlns:trt="{MEDIA_NS}" xmlns:tptz="{PTZ_NS}">{header}<s:Body>{body}</s:Body></s:Envelope>"#
    )
}

/// Format seconds since the epoch as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn iso8601_utc(secs: u64) -> String {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(i64::try_from(days).unwrap_or(0));
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 → (year, month, day).  Howard Hinnant's algorithm.
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

/// 16 nonce bytes from the process-random `RandomState` keys and the clock.
///
/// The WS-Security nonce exists to defeat replay, so it must not repeat; it
/// does not need to be a cryptographic secret.
fn fresh_nonce() -> [u8; 16] {
    use std::hash::{BuildHasher, Hasher};
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut out = [0u8; 16];
    for (i, half) in out.chunks_mut(8).enumerate() {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(now);
        h.write_usize(i);
        half.copy_from_slice(&h.finish().to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_digest_is_base64_sha1_of_nonce_created_password() {
        // SHA1("abc" ‖ "2026-01-01T00:00:00Z" ‖ "pw"), computed independently.
        let d = password_digest(b"abc", "2026-01-01T00:00:00Z", "pw");
        let mut h = Sha1::new();
        h.update(b"abc2026-01-01T00:00:00Zpw");
        assert_eq!(
            d,
            base64::engine::general_purpose::STANDARD.encode(h.finalize())
        );
        assert_eq!(d.len(), 28, "base64 of a 20-byte digest");
    }

    #[test]
    fn security_header_carries_the_token_and_escapes_the_username() {
        let h = security_header("a&b", "pw", b"0123456789abcdef", "2026-10-23T00:00:00Z");
        assert!(h.contains("<wsse:Username>a&amp;b</wsse:Username>"));
        assert!(h.contains("#PasswordDigest"));
        assert!(h.contains("<wsu:Created>2026-10-23T00:00:00Z</wsu:Created>"));
        assert!(h.contains("MDEyMzQ1Njc4OWFiY2RlZg=="), "base64 nonce: {h}");
        assert!(!h.contains(">pw<"), "the plain password must never be sent");
    }

    #[test]
    fn iso8601_formats_known_instants() {
        assert_eq!(iso8601_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso8601_utc(1_792_713_600 + 3_661), "2026-10-23T01:01:01Z");
    }

    #[test]
    fn fresh_nonces_differ() {
        assert_ne!(fresh_nonce(), fresh_nonce());
    }

    #[test]
    fn envelope_declares_every_prefix_and_places_the_header() {
        let e = envelope(Some("<h/>"), "<tds:GetDeviceInformation/>");
        for ns in [SOAP_ENV_NS, SCHEMA_NS, DEVICE_NS, MEDIA_NS, PTZ_NS] {
            assert!(e.contains(ns));
        }
        assert!(e.contains("<s:Header><h/></s:Header><s:Body><tds:GetDeviceInformation/></s:Body>"));
        assert!(!envelope(None, "<x/>").contains("Header"));
    }
}
