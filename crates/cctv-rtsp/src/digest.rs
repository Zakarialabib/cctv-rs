//! RTSP Digest authentication state machine and challenge parser.
//!
//! Handles `401 Unauthorized` challenges carrying `WWW-Authenticate: Digest`
//! headers, and generates RFC 2617 / RFC 7616 `Authorization: Digest` headers.
//!
//! Supports both legacy RFC 2617 without quality-of-protection
//! (`response = MD5(HA1:nonce:HA2)`) and `qop="auth"`
//! (`response = MD5(HA1:nonce:nc:cnonce:qop:HA2)`), pulling the password from
//! [`DecryptedCameraAuth`]'s zero-on-drop `Zeroizing<String>`.

use cctv_core::DecryptedCameraAuth;
use md5::{Digest as _, Md5};

/// The parameters extracted from a `WWW-Authenticate: Digest ...` challenge.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DigestChallenge {
    /// Realm string protecting the resource.
    pub realm: String,
    /// Server-specified nonce string.
    pub nonce: String,
    /// Opaque string to be passed back unchanged.
    pub opaque: Option<String>,
    /// Cryptographic algorithm specified (e.g. `MD5`).
    pub algorithm: Option<String>,
    /// Whether the server specifies `qop="auth"` or lists `"auth"` in its qop options.
    pub qop_auth: bool,
}

impl DigestChallenge {
    /// Parse one `WWW-Authenticate` header value into a [`DigestChallenge`].
    /// Returns `None` if the header is not a Digest challenge or is missing a `nonce`.
    pub fn parse(header: &str) -> Option<Self> {
        let header = header.trim();
        let (scheme, params) = header
            .split_once(char::is_whitespace)
            .unwrap_or((header, ""));
        if !scheme.eq_ignore_ascii_case("digest") {
            return None;
        }

        let mut challenge = Self::default();
        for (k, v) in parse_params(params) {
            match k.to_ascii_lowercase().as_str() {
                "realm" => challenge.realm = v,
                "nonce" => challenge.nonce = v,
                "opaque" => challenge.opaque = Some(v),
                "algorithm" => challenge.algorithm = Some(v),
                "qop" => {
                    challenge.qop_auth = v
                        .split(',')
                        .any(|q| q.trim().trim_matches('"').eq_ignore_ascii_case("auth"));
                }
                _ => {}
            }
        }

        if challenge.nonce.is_empty() {
            return None;
        }
        Some(challenge)
    }
}

/// Two-step digest state tracking the challenge and request nonce counter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestState {
    challenge: DigestChallenge,
    nc: u32,
}

impl DigestState {
    /// Create a new digest state machine from a parsed challenge.
    pub fn new(challenge: DigestChallenge) -> Self {
        Self { challenge, nc: 0 }
    }

    /// Attempt to parse a `WWW-Authenticate` header directly into a [`DigestState`].
    pub fn from_header(header: &str) -> Option<Self> {
        DigestChallenge::parse(header).map(Self::new)
    }

    /// Access the underlying challenge.
    pub fn challenge(&self) -> &DigestChallenge {
        &self.challenge
    }

    /// The current nonce count.
    pub fn nonce_count(&self) -> u32 {
        self.nc
    }

    /// Authorize a request against this digest state using credentials from [`DecryptedCameraAuth`].
    ///
    /// Computes:
    /// - `HA1 = MD5(username:realm:password)`
    /// - `HA2 = MD5(method:uri)`
    /// - If `qop="auth"`: increments `nc` and computes `MD5(HA1:nonce:nc:cnonce:auth:HA2)`
    /// - If legacy RFC 2617: computes `MD5(HA1:nonce:HA2)`
    pub fn authorize(
        &mut self,
        username: &str,
        auth: &DecryptedCameraAuth,
        method: &str,
        uri: &str,
        cnonce: &str,
    ) -> String {
        let password = auth.password.as_str();
        let realm = &self.challenge.realm;
        let nonce = &self.challenge.nonce;

        let ha1 = md5_hex(format!("{username}:{realm}:{password}").as_bytes());
        let ha2 = md5_hex(format!("{method}:{uri}").as_bytes());

        let (response, qop_params) = if self.challenge.qop_auth {
            self.nc = self.nc.wrapping_add(1);
            let nc_hex = format!("{:08x}", self.nc);
            let resp = md5_hex(format!("{ha1}:{nonce}:{nc_hex}:{cnonce}:auth:{ha2}").as_bytes());
            let qop_str = format!(r#", qop="auth", nc={nc_hex}, cnonce="{cnonce}""#);
            (resp, qop_str)
        } else {
            let resp = md5_hex(format!("{ha1}:{nonce}:{ha2}").as_bytes());
            (resp, String::new())
        };

        let mut header = format!(
            r#"Digest username="{username}", realm="{realm}", nonce="{nonce}", uri="{uri}", response="{response}""#
        );

        if let Some(algo) = &self.challenge.algorithm {
            header.push_str(&format!(r#", algorithm={algo}"#));
        }

        header.push_str(&qop_params);

        if let Some(opaque) = &self.challenge.opaque {
            header.push_str(&format!(r#", opaque="{opaque}""#));
        }

        header
    }
}

fn md5_hex(data: &[u8]) -> String {
    let mut h = Md5::new();
    h.update(data);
    hex_format(&h.finalize())
}

fn hex_format(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from(HEX[usize::from(b >> 4)]));
        s.push(char::from(HEX[usize::from(b & 0x0F)]));
    }
    s
}

/// Parse comma-separated key=value or key="value" challenge parameters.
fn parse_params(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq].trim().trim_start_matches(',').trim().to_string();
        rest = rest[eq + 1..].trim_start();
        let value;
        if let Some(stripped) = rest.strip_prefix('"') {
            let mut v = String::new();
            let mut chars = stripped.char_indices();
            let mut end = stripped.len();
            while let Some((i, c)) = chars.next() {
                match c {
                    '\\' => {
                        if let Some((_, n)) = chars.next() {
                            v.push(n);
                        }
                    }
                    '"' => {
                        end = i + 1;
                        break;
                    }
                    c => v.push(c),
                }
            }
            value = v;
            rest = stripped.get(end..).unwrap_or("");
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            value = rest[..end].trim().to_string();
            rest = &rest[end..];
        }
        rest = rest.trim_start().trim_start_matches(',').trim_start();
        out.push((key, value));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_challenge_with_qop_auth() {
        let raw = r#"Digest realm="LIVE_STREAM", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", qop="auth", opaque="5ccc069c403ebaf9f0171e9517f40e41""#;
        let challenge = DigestChallenge::parse(raw).expect("a valid challenge");
        assert_eq!(challenge.realm, "LIVE_STREAM");
        assert_eq!(challenge.nonce, "dcd98b7102dd2f0e8b11d0f600bfb0c093");
        assert_eq!(
            challenge.opaque.as_deref(),
            Some("5ccc069c403ebaf9f0171e9517f40e41")
        );
        assert!(challenge.qop_auth);
    }

    #[test]
    fn parse_challenge_without_nonce_is_none() {
        let raw = r#"Digest realm="LIVE_STREAM""#;
        assert_eq!(DigestChallenge::parse(raw), None);
    }

    #[test]
    fn legacy_rfc_2617_response_calculation() {
        // Worked RFC 2617 example without qop:
        // user="Mufasa", realm="testrealm@host.com", password="Circle Of Life"
        // method="GET", uri="/dir/index.html", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093"
        let auth = DecryptedCameraAuth::new("Circle Of Life", "");
        let challenge = DigestChallenge {
            realm: "testrealm@host.com".to_string(),
            nonce: "dcd98b7102dd2f0e8b11d0f600bfb0c093".to_string(),
            opaque: None,
            algorithm: Some("MD5".to_string()),
            qop_auth: false,
        };
        let mut state = DigestState::new(challenge);

        let header = state.authorize("Mufasa", &auth, "GET", "/dir/index.html", "ignored");
        assert_eq!(state.nonce_count(), 0); // Not incremented when qop is not used

        // HA1 = MD5(Mufasa:testrealm@host.com:Circle Of Life) = 939e7578ed9e3c518a452acee763bce9
        // HA2 = MD5(GET:/dir/index.html) = 39aff3a2bab6126f332b942af96d3366
        // response = MD5(HA1:nonce:HA2) = 670fd8c2df070c60b045671b8b24ff02
        assert!(header.contains(r#"response="670fd8c2df070c60b045671b8b24ff02""#));
        assert!(header.contains(r#"username="Mufasa""#));
        assert!(header.contains(r#"realm="testrealm@host.com""#));
        assert!(!header.contains("qop="));
        assert!(!header.contains("nc="));
    }

    #[test]
    fn qop_auth_response_calculation() {
        // Worked RFC 2617 example with qop="auth":
        // user="Mufasa", realm="testrealm@host.com", password="Circle Of Life"
        // method="GET", uri="/dir/index.html"
        // nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093"
        // cnonce="0a4f113b", nc=1 (formatted 00000001)
        let auth = DecryptedCameraAuth::new("Circle Of Life", "");
        let challenge = DigestChallenge {
            realm: "testrealm@host.com".to_string(),
            nonce: "dcd98b7102dd2f0e8b11d0f600bfb0c093".to_string(),
            opaque: Some("5ccc069c403ebaf9f0171e9517f40e41".to_string()),
            algorithm: Some("MD5".to_string()),
            qop_auth: true,
        };
        let mut state = DigestState::new(challenge);

        let header = state.authorize("Mufasa", &auth, "GET", "/dir/index.html", "0a4f113b");
        assert_eq!(state.nonce_count(), 1);

        // HA1 = 939e7578e77acac5157648b7602f71dd
        // HA2 = 398553363b030138f7972973ae0d125f
        // response = MD5(939e7578e77acac5157648b7602f71dd:dcd98b7102dd2f0e8b11d0f600bfb0c093:00000001:0a4f113b:auth:398553363b030138f7972973ae0d125f)
        //          = 6629fae49393a05397450978507c4ef1
        assert!(header.contains(r#"response="6629fae49393a05397450978507c4ef1""#));
        assert!(header.contains(r#"qop="auth""#));
        assert!(header.contains(r#"nc=00000001"#));
        assert!(header.contains(r#"cnonce="0a4f113b""#));
        assert!(header.contains(r#"opaque="5ccc069c403ebaf9f0171e9517f40e41""#));

        // Subsequent call increments nc
        let header2 = state.authorize("Mufasa", &auth, "GET", "/dir/index.html", "0a4f113b");
        assert_eq!(state.nonce_count(), 2);
        assert!(header2.contains(r#"nc=00000002"#));
    }
}
