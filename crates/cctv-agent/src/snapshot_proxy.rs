//! Native snapshot caching proxy with Digest authentication.
//!
//! Exposes snapshot retrieval proxying a camera's native `snapshot_http_path`.
//! Maintains a 500ms in-memory cache so concurrent or rapid requests
//! neither consume multiple camera RTSP sessions nor burn host CPU transcoding video.

use std::sync::Arc;
use std::time::{Duration, Instant};

use cctv_core::CameraError;
use cctv_transport::http::{HttpClient, HttpConfig, HttpResponse, Method};
use tokio::sync::RwLock;

/// Configuration for snapshot proxy.
#[derive(Debug, Clone)]
pub struct SnapshotProxyConfig {
    /// Native snapshot HTTP path on the device (e.g. `http://cam/onvif/snapshot`).
    pub snapshot_http_path: String,
    /// Username for HTTP Digest / Basic authentication.
    pub username: Option<String>,
    /// Password for HTTP Digest / Basic authentication.
    pub password: Option<String>,
    /// Cache time-to-live (defaults to 500ms).
    pub cache_ttl: Duration,
    /// Accept invalid certificates (e.g. self-signed HTTPS).
    pub accept_invalid_certs: bool,
    /// HTTP timeout.
    pub timeout: Duration,
}

impl SnapshotProxyConfig {
    /// Create a new proxy config with default 500ms cache TTL.
    pub fn new(snapshot_http_path: impl Into<String>) -> Self {
        Self {
            snapshot_http_path: snapshot_http_path.into(),
            username: None,
            password: None,
            cache_ttl: Duration::from_millis(500),
            accept_invalid_certs: false,
            timeout: Duration::from_secs(5),
        }
    }

    /// Builder: set authentication credentials.
    pub fn with_credentials(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.username = Some(username.into());
        self.password = Some(password.into());
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
            cache: Arc::new(RwLock::new(None)),
        })
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
        let config = SnapshotProxyConfig::new(&url).with_credentials("admin", "secret");
        let proxy = SnapshotProxy::new(config).expect("proxy created");

        let snap = proxy.get_snapshot().await.expect("snapshot via digest");
        assert_eq!(snap.bytes, vec![0xAA, 0xBB, 0xCC]);
    }
}
