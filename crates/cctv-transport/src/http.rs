//! The HTTP transport the driver crates share (feature `http`).
//!
//! One thin async client over `reqwest` with **rustls only** (no native-tls).
//! The authentication camera control APIs use — HTTP Digest (RFC 7616,
//! MD5 / SHA-256, `qop=auth`) and Basic, negotiated from the
//! `WWW-Authenticate` challenge on a 401 — is computed in [`crate::digest`],
//! which is pure and always compiled: this module wires it to the network and
//! nothing more.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use crate::digest::{self, Challenge};

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
            .filter_map(|h| digest::parse_challenge(h))
            .min_by_key(|c| match c {
                Challenge::Digest(_) => 0,
                Challenge::Basic => 1,
            })
        else {
            return Ok(first.0);
        };
        let authorization = match challenge {
            Challenge::Basic => digest::basic_authorization(user, pass),
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
                digest::digest_authorization(
                    &d,
                    method.as_str(),
                    &target,
                    user,
                    pass,
                    &digest::client_nonce(),
                    nc,
                )
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
