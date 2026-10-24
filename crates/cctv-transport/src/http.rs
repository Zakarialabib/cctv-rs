//! The HTTP transport the driver crates share (feature `http`).
//!
//! One thin async client over `reqwest` with **rustls only** (no native-tls),
//! plus the authentication camera control APIs actually use: HTTP Digest
//! (RFC 7616, MD5 / SHA-256, `qop=auth`) and Basic, negotiated from the
//! `WWW-Authenticate` challenge on a 401.
//!
//! The digest computation and the challenge parser are pure functions, tested
//! against the RFC 2617 worked example; [`HttpClient`] only wires them to the
//! network.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use base64::Engine as _;
use md5::Md5;
use sha2::{Digest as _, Sha256};

/// Failure of an HTTP exchange, before any status-code interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    /// The client could not be built (TLS backend, invalid option).
    Build(String),
    /// The request URL is invalid.
    InvalidUrl(String),
    /// Connecting failed (DNS, refused, unreachable, TLS handshake).
    Connect(String),
    /// The request exceeded its timeout.
    Timeout,
    /// The response body could not be read.
    Body(String),
}

impl core::fmt::Display for HttpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Build(e) => write!(f, "http client build failed: {e}"),
            Self::InvalidUrl(e) => write!(f, "invalid url: {e}"),
            Self::Connect(e) => write!(f, "connect failed: {e}"),
            Self::Timeout => write!(f, "request timed out"),
            Self::Body(e) => write!(f, "reading the response failed: {e}"),
        }
    }
}

impl std::error::Error for HttpError {}

/// HTTP method subset the camera APIs use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// `GET`.
    Get,
    /// `POST`.
    Post,
    /// `PUT`.
    Put,
    /// `DELETE`.
    Delete,
}

impl Method {
    /// The method token as sent on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
        }
    }
}

/// A completed HTTP exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// Status code.
    pub status: u16,
    /// `Content-Type` header, if any.
    pub content_type: Option<String>,
    /// Raw body bytes.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// The body as text (lossy UTF-8).
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Whether the status is 2xx.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Client options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpConfig {
    /// Per-request timeout.
    pub timeout: Duration,
    /// Accept self-signed / mismatched certificates.  Cameras commonly ship
    /// self-signed HTTPS; this is off by default and must be opted into.
    pub accept_invalid_certs: bool,
    /// Username for Digest / Basic authentication.
    pub username: Option<String>,
    /// Password for Digest / Basic authentication.
    pub password: Option<String>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            accept_invalid_certs: false,
            username: None,
            password: None,
        }
    }
}

/// The shared async HTTP client with Digest / Basic negotiation.
#[derive(Debug)]
pub struct HttpClient {
    inner: reqwest::Client,
    username: Option<String>,
    password: Option<String>,
    nonce_count: AtomicU32,
}

impl HttpClient {
    /// Build a client.
    pub fn new(config: HttpConfig) -> Result<Self, HttpError> {
        let inner = reqwest::Client::builder()
            .timeout(config.timeout)
            .danger_accept_invalid_certs(config.accept_invalid_certs)
            .build()
            .map_err(|e| HttpError::Build(e.to_string()))?;
        Ok(Self {
            inner,
            username: config.username,
            password: config.password,
            nonce_count: AtomicU32::new(0),
        })
    }

    /// Send one request.  On a 401 carrying a Digest or Basic challenge, and
    /// when credentials are configured, the request is retried once with the
    /// answer.  The final response is returned whatever its status — mapping
    /// status codes to domain errors is the driver's job.
    pub async fn request(
        &self,
        method: Method,
        url: &str,
        content_type: Option<&str>,
        body: Option<Vec<u8>>,
    ) -> Result<HttpResponse, HttpError> {
        let parsed = reqwest::Url::parse(url).map_err(|e| HttpError::InvalidUrl(e.to_string()))?;
        let first = self
            .send(method, parsed.clone(), content_type, body.clone(), None)
            .await?;
        if first.0.status != 401 {
            return Ok(first.0);
        }
        let Some(user) = self.username.as_deref() else {
            return Ok(first.0);
        };
        let pass = self.password.as_deref().unwrap_or("");
        let Some(challenge) = first
            .1
            .iter()
            .filter_map(|h| parse_challenge(h))
            .min_by_key(|c| match c {
                Challenge::Digest(_) => 0,
                Challenge::Basic => 1,
            })
        else {
            return Ok(first.0);
        };
        let authorization = match challenge {
            Challenge::Basic => basic_authorization(user, pass),
            Challenge::Digest(d) => {
                let mut target = parsed.path().to_string();
                if let Some(q) = parsed.query() {
                    target.push('?');
                    target.push_str(q);
                }
                let nc = self
                    .nonce_count
                    .fetch_add(1, Ordering::Relaxed)
                    .wrapping_add(1);
                digest_authorization(&d, method.as_str(), &target, user, pass, &cnonce(), nc)
            }
        };
        let second = self
            .send(method, parsed, content_type, body, Some(authorization))
            .await?;
        Ok(second.0)
    }

    async fn send(
        &self,
        method: Method,
        url: reqwest::Url,
        content_type: Option<&str>,
        body: Option<Vec<u8>>,
        authorization: Option<String>,
    ) -> Result<(HttpResponse, Vec<String>), HttpError> {
        let m = match method {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
            Method::Put => reqwest::Method::PUT,
            Method::Delete => reqwest::Method::DELETE,
        };
        let mut req = self.inner.request(m, url);
        if let Some(ct) = content_type {
            req = req.header(reqwest::header::CONTENT_TYPE, ct);
        }
        if let Some(a) = authorization {
            req = req.header(reqwest::header::AUTHORIZATION, a);
        }
        if let Some(b) = body {
            req = req.body(b);
        }
        let resp = req.send().await.map_err(|e| {
            if e.is_timeout() {
                HttpError::Timeout
            } else {
                HttpError::Connect(e.to_string())
            }
        })?;
        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let challenges = resp
            .headers()
            .get_all(reqwest::header::WWW_AUTHENTICATE)
            .iter()
            .filter_map(|v| v.to_str().ok().map(str::to_string))
            .collect();
        let body = resp
            .bytes()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    HttpError::Timeout
                } else {
                    HttpError::Body(e.to_string())
                }
            })?
            .to_vec();
        Ok((
            HttpResponse {
                status,
                content_type,
                body,
            },
            challenges,
        ))
    }
}

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

/// Compute the `Authorization: Digest ...` header value (RFC 7616).
///
/// Supports `MD5`, `MD5-sess`, `SHA-256`, `SHA-256-sess`; with or without
/// `qop=auth`.  `nc` is the nonce count (formatted as 8 hex digits).
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

/// The `Authorization: Basic ...` header value.
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
}
