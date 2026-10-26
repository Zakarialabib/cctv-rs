//! HTTP Digest authentication: the challenge parser and the `Authorization`
//! header (RFC 7616, MD5 / SHA-256, `qop=auth`).
//!
//! **Why this module is not behind a feature.** It is pure hashing: two inputs
//! in, one header string out, no socket and no client. RTSP needs it exactly as
//! much as HTTP does — a camera's DESCRIBE answers `401` with the same challenge
//! and expects the same response — but the RTSP driver must not link `reqwest`,
//! and this used to live in the `http` module, which *is* `reqwest`. A pure
//! helper gated behind the client it does not need is a dependency trap, so the
//! computation moved here and the client imports it.
//!
//! **Why the header computation lives here at all.** The algorithm is identical
//! for both protocols; only the method token and the request target differ
//! (`DESCRIBE /stream` rather than `GET /dir/index.html`). One implementation,
//! tested against the RFC 2617 worked example, serves both rather than two
//! copies drifting.
//!
//! `qop=auth` is supported; `qop=auth-int` is not, because the body hash makes
//! the response depend on a payload a stream has no business carrying.

use base64::Engine as _;
use md5::Md5;
use sha2::{Digest as _, Sha256};

/// A parsed `WWW-Authenticate` challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Challenge {
    /// `Basic realm=...`.
    Basic,
    /// `Digest ...`.
    Digest(DigestChallenge),
}

/// The parameters of a Digest challenge.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DigestChallenge {
    /// `realm`.
    pub realm: String,
    /// `nonce`.
    pub nonce: String,
    /// `opaque`, echoed back verbatim.
    pub opaque: Option<String>,
    /// `algorithm` (`MD5` when absent).
    pub algorithm: Option<String>,
    /// Whether the server offers `qop=auth`.
    pub qop_auth: bool,
}

/// Parse one `WWW-Authenticate` header value.  Unknown schemes yield `None`.
///
/// A `Digest` challenge with no `nonce` is not a challenge — it is noise — so it
/// yields `None` rather than a header that can never be answered.
pub fn parse_challenge(header: &str) -> Option<Challenge> {
    let header = header.trim();
    let (scheme, params) = header
        .split_once(char::is_whitespace)
        .unwrap_or((header, ""));
    if scheme.eq_ignore_ascii_case("basic") {
        return Some(Challenge::Basic);
    }
    if !scheme.eq_ignore_ascii_case("digest") {
        return None;
    }
    let mut d = DigestChallenge::default();
    for (k, v) in parse_params(params) {
        match k.to_ascii_lowercase().as_str() {
            "realm" => d.realm = v,
            "nonce" => d.nonce = v,
            "opaque" => d.opaque = Some(v),
            "algorithm" => d.algorithm = Some(v),
            "qop" => d.qop_auth = v.split(',').any(|q| q.trim().eq_ignore_ascii_case("auth")),
            _ => {}
        }
    }
    if d.nonce.is_empty() {
        return None;
    }
    Some(Challenge::Digest(d))
}

/// Split `k=v, k2="v, with comma"` into pairs, honouring quoted strings.
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

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from(HEX[usize::from(b >> 4)]));
        s.push(char::from(HEX[usize::from(b & 0x0F)]));
    }
    s
}

fn hash(algorithm: &str, data: &str) -> String {
    if algorithm.to_ascii_uppercase().starts_with("SHA-256") {
        hex(&Sha256::digest(data.as_bytes()))
    } else {
        hex(&Md5::digest(data.as_bytes()))
    }
}

/// Compute the `Authorization: Digest *** header value (RFC 7616).
///
/// Supports `MD5`, `MD5-sess`, `SHA-256`, `SHA-256-sess`; with or without
/// `qop=auth`.  `nc` is the nonce count (formatted as 8 hex digits).
///
/// `method` and `uri` are the caller's, because the HA2 line is
/// `method:uri`: an HTTP client passes `GET` and a path, an RTSP client passes
/// `DESCRIBE` and the session URI.  Nothing here assumes either.
pub fn digest_authorization(
    ch: &DigestChallenge,
    method: &str,
    uri: &str,
    username: &str,
    password: &str,
    cnonce: &str,
    nc: u32,
) -> String {
    let algorithm = ch.algorithm.clone().unwrap_or_else(|| "MD5".to_string());
    let mut ha1 = hash(&algorithm, &format!("{username}:{}:{password}", ch.realm));
    if algorithm.to_ascii_lowercase().ends_with("-sess") {
        ha1 = hash(&algorithm, &format!("{ha1}:{}:{cnonce}", ch.nonce));
    }
    let ha2 = hash(&algorithm, &format!("{method}:{uri}"));
    let nc = format!("{nc:08x}");
    let response = if ch.qop_auth {
        hash(
            &algorithm,
            &format!("{ha1}:{}:{nc}:{cnonce}:auth:{ha2}", ch.nonce),
        )
    } else {
        hash(&algorithm, &format!("{ha1}:{}:{ha2}", ch.nonce))
    };
    let mut header = format!(
        "Digest username=\"{username}\", realm=\"{}\", nonce=\"{}\", uri=\"{uri}\", algorithm={algorithm}, response=\"{response}\"",
        ch.realm, ch.nonce
    );
    if ch.qop_auth {
        header.push_str(&format!(", qop=auth, nc={nc}, cnonce=\"{cnonce}\""));
    }
    if let Some(o) = &ch.opaque {
        header.push_str(&format!(", opaque=\"{o}\""));
    }
    header
}

/// The `Authorization: Basic *** header value.
pub fn basic_authorization(username: &str, password: &str) -> String {
    let token = base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
    format!("Basic {token}")
}

/// A client nonce from the process-random `RandomState` keys and the clock.
///
/// Digest only needs the client nonce to be unpredictable per request, not
/// cryptographically strong; this avoids a dependency on an RNG crate.
fn cnonce() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    if let Ok(d) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        h.write_u128(d.as_nanos());
    }
    format!("{:016x}", h.finish())
}

/// A client nonce for the *next* request, fresh each call.
///
/// The `HttpClient` in `cctv-transport::http` owns its own nonce counter; an
/// RTSP session with no counter of its own (see `cctv-rtsp`'s `RtspSession`)
/// calls this per request.  Exported for exactly that caller.
pub fn client_nonce() -> String {
    cnonce()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_matches_the_rfc_2617_worked_example() {
        let ch = DigestChallenge {
            realm: "testrealm@host.com".into(),
            nonce: "dcd98b7102dd2f0e8b11d0f600bfb0c093".into(),
            opaque: Some("5ccc069c403ebaf9f0171e9517f40e41".into()),
            algorithm: None,
            qop_auth: true,
        };
        let h = digest_authorization(
            &ch,
            "GET",
            "/dir/index.html",
            "Mufasa",
            "Circle Of Life",
            "0a4f113b",
            1,
        );
        assert!(
            h.contains("response=\"6629fae49393a05397450978507c4ef1\""),
            "{h}"
        );
        assert!(h.contains("nc=00000001"));
        assert!(h.contains("opaque=\"5ccc069c403ebaf9f0171e9517f40e41\""));
    }

    #[test]
    fn challenges_parse_quoted_commas_and_pick_qop_auth() {
        let c = parse_challenge(
            r#"Digest realm="IP Camera(C1234)", qop="auth,auth-int", nonce="abc,def", opaque="o", algorithm=MD5"#,
        );
        let Some(Challenge::Digest(d)) = c else {
            panic!("not digest: {c:?}");
        };
        assert_eq!(d.realm, "IP Camera(C1234)");
        assert_eq!(d.nonce, "abc,def");
        assert!(d.qop_auth);
        assert_eq!(d.algorithm.as_deref(), Some("MD5"));
        assert_eq!(parse_challenge("Basic realm=\"x\""), Some(Challenge::Basic));
        assert_eq!(parse_challenge("Bearer x"), None);
        assert_eq!(parse_challenge("Digest realm=\"x\""), None, "no nonce");
    }

    #[test]
    fn basic_auth_is_base64_of_user_colon_password() {
        assert_eq!(
            basic_authorization("Aladdin", "open sesame"),
            "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
    }

    #[test]
    fn sha256_digest_differs_from_md5() {
        let mut ch = DigestChallenge {
            realm: "r".into(),
            nonce: "n".into(),
            ..Default::default()
        };
        let md5 = digest_authorization(&ch, "GET", "/", "u", "p", "c", 1);
        ch.algorithm = Some("SHA-256".into());
        let sha = digest_authorization(&ch, "GET", "/", "u", "p", "c", 1);
        assert_ne!(md5, sha);
        assert!(sha.contains("algorithm=SHA-256"));
    }

    #[test]
    fn a_digest_for_rtsp_differs_from_the_same_request_over_http() {
        // The header is a function of the method token and the request target,
        // which is why it is reusable: `DESCRIBE /stream` is not `GET /stream`.
        let ch = DigestChallenge {
            realm: "IP Camera".into(),
            nonce: "abcdef".into(),
            ..DigestChallenge::default()
        };
        let http = digest_authorization(&ch, "GET", "/stream", "admin", "pw", "c", 1);
        let rtsp = digest_authorization(&ch, "DESCRIBE", "/stream", "admin", "pw", "c", 1);
        assert_ne!(http, rtsp, "the HA2 line carries the method");
    }
}
