//! QR URI format for camera / host pairing:
//! URI format:
//! `cctv://pair?v=0&id=<zbase32>&s=<b64url>&n=<pct-encoded>&a=<ip:port>,...`

use std::fmt;
use std::net::SocketAddr;
use std::str::FromStr;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use zeroize::Zeroizing;

use crate::zbase32;

/// Error parsing a pairing URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QrUriError {
    /// Invalid scheme (must start with cctv://pair?).
    InvalidScheme,
    /// Missing required query parameter.
    MissingParam(&'static str),
    /// Unsupported protocol version.
    UnsupportedVersion(u32),
    /// Error decoding z-base-32 ID.
    InvalidId(String),
    /// Error decoding Base64URL shared secret.
    InvalidSecret(String),
    /// Error decoding percent-encoded friendly name.
    InvalidName(String),
    /// Error parsing address list.
    InvalidAddress(String),
}

impl fmt::Display for QrUriError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidScheme => write!(f, "URI must start with 'cctv://pair?'"),
            Self::MissingParam(p) => write!(f, "missing query parameter '{p}'"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported pairing version {v}"),
            Self::InvalidId(e) => write!(f, "invalid z-base-32 device id: {e}"),
            Self::InvalidSecret(e) => write!(f, "invalid base64url secret: {e}"),
            Self::InvalidName(e) => write!(f, "invalid name encoding: {e}"),
            Self::InvalidAddress(e) => write!(f, "invalid socket address: {e}"),
        }
    }
}

impl std::error::Error for QrUriError {}

/// Represents the data carried in a pairing QR code.
#[derive(Clone, PartialEq, Eq)]
pub struct PairingQrPayload {
    /// Pairing protocol version (currently 0).
    pub version: u32,
    /// Device identifier (raw bytes, displayed as z-base-32).
    pub device_id: Vec<u8>,
    /// Ephemeral pre-shared secret (raw bytes, URL-safe base64 encoded).
    pub secret: Zeroizing<Vec<u8>>,
    /// Friendly name of device (percent-encoded in URI).
    pub name: String,
    /// Dialable addresses (IP:port).
    pub addresses: Vec<SocketAddr>,
}

impl fmt::Debug for PairingQrPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairingQrPayload")
            .field("version", &self.version)
            .field("device_id_zbase32", &self.device_id_zbase32())
            .field("secret", &"***REDACTED***")
            .field("name", &self.name)
            .field("addresses", &self.addresses)
            .finish()
    }
}

impl PairingQrPayload {
    /// Create a new pairing payload with version 0.
    pub fn new(
        device_id: Vec<u8>,
        secret: Vec<u8>,
        name: impl Into<String>,
        addresses: Vec<SocketAddr>,
    ) -> Self {
        Self {
            version: 0,
            device_id,
            secret: Zeroizing::new(secret),
            name: name.into(),
            addresses,
        }
    }

    /// Return the device ID encoded as z-base-32.
    pub fn device_id_zbase32(&self) -> String {
        zbase32::encode(&self.device_id)
    }

    /// Return the secret encoded as base64url without padding.
    pub fn secret_b64url(&self) -> String {
        URL_SAFE_NO_PAD.encode(&*self.secret)
    }

    /// Render into the canonical URI representation:
    /// `cctv://pair?v=0&id=<zbase32>&s=<b64url>&n=<pct-encoded>&a=<ip:port>,...`
    pub fn to_uri(&self) -> String {
        let id_str = self.device_id_zbase32();
        let s_str = self.secret_b64url();
        let n_str = percent_encode_str(&self.name);
        let addrs_str = self
            .addresses
            .iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join(",");

        format!(
            "cctv://pair?v={}&id={}&s={}&n={}&a={}",
            self.version, id_str, s_str, n_str, addrs_str
        )
    }
}

impl FromStr for PairingQrPayload {
    type Err = QrUriError;

    fn from_str(uri: &str) -> Result<Self, Self::Err> {
        let prefix = "cctv://pair?";
        if !uri.starts_with(prefix) {
            return Err(QrUriError::InvalidScheme);
        }

        let query = &uri[prefix.len()..];
        let mut version: Option<u32> = None;
        let mut id: Option<Vec<u8>> = None;
        let mut secret: Option<Vec<u8>> = None;
        let mut name: Option<String> = None;
        let mut addrs: Vec<SocketAddr> = Vec::new();

        for part in query.split('&') {
            if part.is_empty() {
                continue;
            }
            let mut kv = part.splitn(2, '=');
            let key = kv.next().unwrap_or("");
            let val = kv.next().unwrap_or("");

            match key {
                "v" => {
                    let v = val
                        .parse::<u32>()
                        .map_err(|_| QrUriError::UnsupportedVersion(999))?;
                    if v != 0 {
                        return Err(QrUriError::UnsupportedVersion(v));
                    }
                    version = Some(v);
                }
                "id" => {
                    let decoded =
                        zbase32::decode(val).map_err(|e| QrUriError::InvalidId(e.to_string()))?;
                    id = Some(decoded);
                }
                "s" => {
                    let decoded = URL_SAFE_NO_PAD
                        .decode(val)
                        .map_err(|e| QrUriError::InvalidSecret(e.to_string()))?;
                    secret = Some(decoded);
                }
                "n" => {
                    let decoded = percent_decode_str(val)
                        .map_err(|e| QrUriError::InvalidName(e.to_string()))?;
                    name = Some(decoded);
                }
                "a" => {
                    for item in val.split(',') {
                        let item = item.trim();
                        if item.is_empty() {
                            continue;
                        }
                        let sa = item
                            .parse::<SocketAddr>()
                            .map_err(|e| QrUriError::InvalidAddress(format!("{item}: {e}")))?;
                        addrs.push(sa);
                    }
                }
                _ => {
                    // Ignore unrecognized query parameters for forward compatibility
                }
            }
        }

        let version = version.ok_or(QrUriError::MissingParam("v"))?;
        let device_id = id.ok_or(QrUriError::MissingParam("id"))?;
        let secret = secret.ok_or(QrUriError::MissingParam("s"))?;
        let name = name.ok_or(QrUriError::MissingParam("n"))?;

        Ok(Self {
            version,
            device_id,
            secret: Zeroizing::new(secret),
            name,
            addresses: addrs,
        })
    }
}

/// Simple percent-encode for friendly names (encodes anything outside ASCII alphanumeric, '-', '_', '.', '~').
fn percent_encode_str(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{b:02X}"));
            }
        }
    }
    out
}

/// Simple percent-decode for friendly names.
fn percent_decode_str(input: &str) -> Result<String, &'static str> {
    let mut bytes = Vec::with_capacity(input.len());
    let mut chars = input.bytes();

    while let Some(b) = chars.next() {
        if b == b'%' {
            let h1 = chars.next().ok_or("incomplete percent escape")?;
            let h2 = chars.next().ok_or("incomplete percent escape")?;
            let hex_buf = [h1, h2];
            let hex_str = std::str::from_utf8(&hex_buf).map_err(|_| "invalid utf8 hex")?;
            let byte = u8::from_str_radix(hex_str, 16).map_err(|_| "invalid hex character")?;
            bytes.push(byte);
        } else {
            bytes.push(b);
        }
    }

    String::from_utf8(bytes).map_err(|_| "invalid UTF-8 in decoded name")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qr_uri_roundtrip() {
        let dev_id = vec![0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc];
        let secret = vec![0xca, 0xfe, 0xba, 0xbe, 0x01, 0x02, 0x03, 0x04];
        let name = "Front Door Camera / Yard #1";
        let addrs: Vec<SocketAddr> = vec![
            "192.168.1.50:8000".parse().unwrap(),
            "[::1]:8000".parse().unwrap(),
        ];

        let payload = PairingQrPayload::new(dev_id.clone(), secret.clone(), name, addrs.clone());
        let uri = payload.to_uri();

        assert!(uri.starts_with("cctv://pair?v=0&id="));
        assert!(uri.contains("&s="));
        assert!(uri.contains("&n=Front%20Door%20Camera%20%2F%20Yard%20%231"));
        assert!(uri.contains("&a=192.168.1.50:8000,[::1]:8000"));

        let parsed: PairingQrPayload = uri.parse().expect("failed to parse valid uri");
        assert_eq!(parsed.version, 0);
        assert_eq!(parsed.device_id, dev_id);
        assert_eq!(*parsed.secret, secret);
        assert_eq!(parsed.name, name);
        assert_eq!(parsed.addresses, addrs);
    }

    #[test]
    fn test_qr_uri_missing_param() {
        assert_eq!(
            "cctv://pair?v=0&s=AAAA&n=Cam"
                .parse::<PairingQrPayload>()
                .unwrap_err(),
            QrUriError::MissingParam("id")
        );
    }

    #[test]
    fn test_qr_uri_invalid_scheme() {
        assert_eq!(
            "https://example.com/pair?v=0"
                .parse::<PairingQrPayload>()
                .unwrap_err(),
            QrUriError::InvalidScheme
        );
    }
}
