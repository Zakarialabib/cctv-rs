//! Native snapshot caching proxy with Digest authentication.
//!
//! Exposes snapshot retrieval proxying a camera's native `snapshot_http_path`.
//! Maintains a 500ms in-memory cache so concurrent or rapid requests
//! neither consume multiple camera RTSP sessions nor burn host CPU transcoding video.

use std::sync::Arc;
use std::time::{Duration, Instant};

use cctv_core::{CameraError, DecryptedCameraAuth, StreamProfileSet};
use cctv_rtsp::DigestState;
use cctv_transport::http::{HttpClient, HttpConfig, HttpResponse, Method};
use tokio::sync::RwLock;

/// Configuration for snapshot proxy.
#[derive(Clone)]
pub struct SnapshotProxyConfig {
    /// Native snapshot HTTP path on the device (e.g. `http://cam/onvif/snapshot`).
    pub snapshot_http_path: String,
    /// Username for HTTP Digest / Basic authentication.
    pub username: Option<String>,
    /// Password for HTTP Digest / Basic authentication.
    pub password: Option<String>,
    /// Zeroizing authentication credentials.
    pub auth: Option<DecryptedCameraAuth>,
    /// Cache time-to-live (defaults to 500ms).
    pub cache_ttl: Duration,
    /// Accept invalid certificates (e.g. self-signed HTTPS).
    pub accept_invalid_certs: bool,
    /// HTTP timeout.
    pub timeout: Duration,
}

impl core::fmt::Debug for SnapshotProxyConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SnapshotProxyConfig")
            .field("snapshot_http_path", &self.snapshot_http_path)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .field("auth", &self.auth)
            .field("cache_ttl", &self.cache_ttl)
            .field("accept_invalid_certs", &self.accept_invalid_certs)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl SnapshotProxyConfig {
    /// Create a new proxy config with default 500ms cache TTL.
    pub fn new(snapshot_http_path: impl Into<String>) -> Self {
        Self {
            snapshot_http_path: snapshot_http_path.into(),
            username: None,
            password: None,
            auth: None,
            cache_ttl: Duration::from_millis(500),
            accept_invalid_certs: false,
            timeout: Duration::from_secs(5),
        }
    }

    /// Create from a camera's `StreamProfileSet` and optional base URL.
    pub fn from_profile_set(
        base_url: &str,
        profile_set: &StreamProfileSet,
    ) -> Result<Self, CameraError> {
        let snapshot_path = profile_set.snapshot_http_path.as_deref().ok_or_else(|| {
            CameraError::Other("no snapshot_http_path defined in profile set".into())
        })?;

        let url = if snapshot_path.starts_with("http://") || snapshot_path.starts_with("https://") {
            snapshot_path.to_string()
        } else {
            format!(
                "{}/{}",
                base_url.trim_end_matches('/'),
                snapshot_path.trim_start_matches('/')
            )
        };

        Ok(Self::new(url))
    }

    /// Builder: set authentication credentials.
    pub fn with_credentials(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        let pw = password.into();
        self.username = Some(username.into());
        self.auth = Some(DecryptedCameraAuth::new(pw.clone(), ""));
        self.password = Some(pw);
        self
    }

    /// Builder: set decrypted auth envelope.
    pub fn with_auth(mut self, username: impl Into<String>, auth: DecryptedCameraAuth) -> Self {
        self.username = Some(username.into());
        self.password = Some(auth.password.as_str().to_string());
        self.auth = Some(auth);
        self
    }
}

/// A cached snapshot image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedSnapshot {
    /// Raw snapshot image bytes (typically JPEG).
    pub bytes: Vec<u8>,
    /// Content-Type header (e.g. `image/jpeg`).
    pub content_type: String,
    /// When this snapshot was captured.
    pub fetched_at: Instant,
}

/// In-memory caching proxy for a camera's native HTTP snapshot.
pub struct SnapshotProxy {
    config: SnapshotProxyConfig,
    client: HttpClient,
    digest_state: Arc<RwLock<Option<DigestState>>>,
    cache: Arc<RwLock<Option<CachedSnapshot>>>,
}

impl SnapshotProxy {
    /// Create a new `SnapshotProxy` for the given camera configuration.
    pub fn new(config: SnapshotProxyConfig) -> Result<Self, CameraError> {
        let http_config = HttpConfig {
            timeout: config.timeout,
            accept_invalid_certs: config.accept_invalid_certs,
            username: config.username.clone(),
            password: config.password.clone(),
        };
        let client = HttpClient::new(http_config)
            .map_err(|e| CameraError::Other(format!("failed to initialize HTTP client: {e}")))?;

        Ok(Self {
            config,
            client,
            digest_state: Arc::new(RwLock::new(None)),
            cache: Arc::new(RwLock::new(None)),
        })
    }

    /// Access the underlying `DigestState`, if challenged.
    pub async fn digest_state(&self) -> Option<DigestState> {
        self.digest_state.read().await.clone()
    }

    /// Get a snapshot (for `GET /agent/cameras/{id}/snapshot`), either from the 500ms in-memory cache
    /// or by querying the device native HTTP path with Digest auth.
    pub async fn get_snapshot(&self) -> Result<CachedSnapshot, CameraError> {
        {
            let read = self.cache.read().await;
            if let Some(cached) = &*read {
                if cached.fetched_at.elapsed() < self.config.cache_ttl {
                    return Ok(cached.clone());
                }
            }
        }

        // Cache miss or expired: fetch from camera native HTTP path
        // HttpClient automatically handles 401 WWW-Authenticate challenges using Digest / Basic auth.
        // We also track DigestState for inspection / state machine compatibility.
        let resp: HttpResponse = self
            .client
            .request(Method::Get, &self.config.snapshot_http_path, None, None)
            .await
            .map_err(|e| CameraError::Other(format!("snapshot request failed: {e}")))?;

        if !resp.is_success() {
            return Err(CameraError::Other(format!(
                "camera snapshot returned HTTP status {}",
                resp.status
            )));
        }

        let content_type = resp
            .content_type
            .unwrap_or_else(|| "image/jpeg".to_string());
        let snapshot = CachedSnapshot {
            bytes: resp.body,
            content_type,
            fetched_at: Instant::now(),
        };

        {
            let mut write = self.cache.write().await;
            *write = Some(snapshot.clone());
        }

        Ok(snapshot)
    }

    /// Record a Digest challenge into the proxy's internal `DigestState`.
    pub async fn update_digest_challenge(&self, challenge_header: &str) {
        if let Some(state) = DigestState::from_header(challenge_header) {
            let mut write = self.digest_state.write().await;
            *write = Some(state);
        }
    }

    /// Invalidate the current in-memory cache.
    pub async fn invalidate(&self) {
        let mut write = self.cache.write().await;
        *write = None;
    }

    /// Read the cached snapshot without triggering an upstream network request.
    pub async fn peek_cached(&self) -> Option<CachedSnapshot> {
        self.cache.read().await.clone()
    }
}

/// Helper to extract path and query from an absolute URL or path.
pub fn uri_path(url_or_path: &str) -> &str {
    if let Some(idx) = url_or_path.find("://") {
        let rest = &url_or_path[idx + 3..];
        match rest.find('/') {
            Some(p_idx) => &rest[p_idx..],
            None => "/",
        }
    } else {
        url_or_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cctv_transport::fixture::{FixtureServer, Route};

    #[tokio::test]
    async fn snapshot_proxy_caches_for_500ms() {
        let server = FixtureServer::start(vec![Route::bytes(
            "GET",
            "/onvif/snapshot",
            "image/jpeg",
            vec![0xFF, 0xD8, 0xFF, 0xE0, 0x01, 0x02],
        )])
        .await
        .expect("server started");

        let url = format!("{}/onvif/snapshot", server.url());
        let config = SnapshotProxyConfig::new(&url);
        let proxy = SnapshotProxy::new(config).expect("proxy created");

        // First request: cache miss, fetches from server
        let snap1 = proxy.get_snapshot().await.expect("snapshot 1");
        assert_eq!(snap1.bytes, vec![0xFF, 0xD8, 0xFF, 0xE0, 0x01, 0x02]);
        assert_eq!(snap1.content_type, "image/jpeg");

        // Second request immediately: served from 500ms cache
        let snap2 = proxy.get_snapshot().await.expect("snapshot 2");
        assert_eq!(snap1.fetched_at, snap2.fetched_at);

        // Server only recorded 1 GET request
        let requests = server.requests();
        let snapshot_requests = requests
            .iter()
            .filter(|r| r.method == "GET" && r.target == "/onvif/snapshot")
            .count();
        assert_eq!(snapshot_requests, 1);
    }

    #[tokio::test]
    async fn snapshot_proxy_works_with_digest_auth() {
        let server = FixtureServer::start_with_digest(
            vec![Route::bytes(
                "GET",
                "/snapshot.jpg",
                "image/jpeg",
                vec![0xAA, 0xBB, 0xCC],
            )],
            "admin",
        )
        .await
        .expect("digest server started");

        let url = format!("{}/snapshot.jpg", server.url());
        let auth = DecryptedCameraAuth::new("secret", "");
        let config = SnapshotProxyConfig::new(&url).with_auth("admin", auth);
        let proxy = SnapshotProxy::new(config).expect("proxy created");

        let snap = proxy.get_snapshot().await.expect("snapshot via digest");
        assert_eq!(snap.bytes, vec![0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn snapshot_proxy_config_from_stream_profile_set() {
        let profiles = StreamProfileSet {
            main_profile_token: "p1".to_string(),
            main_rtsp_path: "/live1".to_string(),
            sub_profile_token: None,
            sub_rtsp_path: None,
            snapshot_http_path: Some("/onvif/snapshot".to_string()),
        };

        let config = SnapshotProxyConfig::from_profile_set("http://192.168.1.10:80", &profiles)
            .expect("config created");
        assert_eq!(
            config.snapshot_http_path,
            "http://192.168.1.10:80/onvif/snapshot"
        );
    }
}
