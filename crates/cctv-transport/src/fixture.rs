//! A canned-response HTTP server for driver tests (feature `fixture`).
//!
//! Drivers prove the conformance suite against this server instead of a live
//! camera: it serves fixed responses chosen by method, path and (for SOAP)
//! a body substring, records every request, and can demand HTTP Digest
//! authentication so the auth path is exercised too.  It holds no
//! credentials of any real device.
//!
//! HTTP/1.1 only, one request per connection (`Connection: close`).

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// One canned response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// Method to match (`GET`, `PUT`, ...), or `*` for any.
    pub method: String,
    /// Request target to match.  A trailing `*` makes it a prefix match;
    /// otherwise the path must match exactly (the query string is ignored
    /// unless the pattern contains `?`).
    pub path: String,
    /// When set, the request body must contain this substring (selects a SOAP
    /// action on a single endpoint).
    pub body_contains: Option<String>,
    /// Response status.
    pub status: u16,
    /// Response `Content-Type`.
    pub content_type: String,
    /// Response body.
    pub body: Vec<u8>,
}

impl Route {
    /// A `200` route returning XML.
    pub fn xml(method: &str, path: &str, body: &str) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            body_contains: None,
            status: 200,
            content_type: "application/xml".into(),
            body: body.as_bytes().to_vec(),
        }
    }

    /// A `200` route returning plain text.
    pub fn text(method: &str, path: &str, body: &str) -> Self {
        Self {
            content_type: "text/plain".into(),
            ..Self::xml(method, path, body)
        }
    }

    /// A `200` route returning arbitrary bytes with a content type.
    pub fn bytes(method: &str, path: &str, content_type: &str, body: Vec<u8>) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            body_contains: None,
            status: 200,
            content_type: content_type.into(),
            body,
        }
    }

    /// A SOAP route: `POST path`, selected by a body substring (the action).
    pub fn soap(path: &str, action: &str, body: &str) -> Self {
        Self {
            body_contains: Some(action.into()),
            content_type: "application/soap+xml".into(),
            ..Self::xml("POST", path, body)
        }
    }

    /// Override the status.
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    fn matches(&self, req: &RecordedRequest) -> bool {
        if self.method != "*" && !self.method.eq_ignore_ascii_case(&req.method) {
            return false;
        }
        let target = if self.path.contains('?') {
            req.target.as_str()
        } else {
            req.target.split('?').next().unwrap_or("")
        };
        let path_ok = match self.path.strip_suffix('*') {
            Some(prefix) => target.starts_with(prefix),
            None => target == self.path,
        };
        path_ok
            && self
                .body_contains
                .as_ref()
                .is_none_or(|needle| req.body.contains(needle.as_str()))
    }
}

/// A request the fixture received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedRequest {
    /// Method token.
    pub method: String,
    /// Request target (path and query).
    pub target: String,
    /// Headers, names lower-cased.
    pub headers: Vec<(String, String)>,
    /// Body as text (lossy).
    pub body: String,
}

impl RecordedRequest {
    /// First header with this (case-insensitive) name.
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.as_str())
    }
}

struct Shared {
    routes: Vec<Route>,
    digest_user: Option<String>,
    requests: Mutex<Vec<RecordedRequest>>,
}

/// A running fixture server.  Stops when dropped.
pub struct FixtureServer {
    addr: SocketAddr,
    shared: Arc<Shared>,
    task: tokio::task::JoinHandle<()>,
}

impl FixtureServer {
    /// Start serving `routes` on an ephemeral `127.0.0.1` port.
    pub async fn start(routes: Vec<Route>) -> std::io::Result<Self> {
        Self::start_inner(routes, None).await
    }

    /// Start serving `routes`, answering any request without a Digest
    /// `Authorization` header for `username` with `401` and a Digest
    /// challenge.
    pub async fn start_with_digest(routes: Vec<Route>, username: &str) -> std::io::Result<Self> {
        Self::start_inner(routes, Some(username.to_string())).await
    }

    async fn start_inner(routes: Vec<Route>, digest_user: Option<String>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared {
            routes,
            digest_user,
            requests: Mutex::new(Vec::new()),
        });
        let s = Arc::clone(&shared);
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let s = Arc::clone(&s);
                tokio::spawn(async move {
                    let _ = serve(stream, &s).await;
                });
            }
        });
        Ok(Self { addr, shared, task })
    }

    /// `http://127.0.0.1:<port>`.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// The bound port.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Every request received so far, in arrival order.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.shared
            .requests
            .lock()
            .map(|r| r.clone())
            .unwrap_or_default()
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn serve(mut stream: TcpStream, shared: &Shared) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
        if buf.len() > 1 << 20 {
            return Ok(());
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next().unwrap_or("").split_whitespace();
    let method = request_line.next().unwrap_or("").to_string();
    let target = request_line.next().unwrap_or("").to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let len = headers
        .iter()
        .find(|(k, _)| k == "content-length")
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    while buf.len() < header_end + len {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body_end = (header_end + len).min(buf.len());
    let req = RecordedRequest {
        method,
        target,
        headers,
        body: String::from_utf8_lossy(&buf[header_end..body_end]).into_owned(),
    };
    if let Ok(mut log) = shared.requests.lock() {
        log.push(req.clone());
    }

    if let Some(user) = &shared.digest_user {
        let authorized = req.header("authorization").is_some_and(|a| {
            a.starts_with("Digest ")
                && a.contains(&format!("username=\"{user}\""))
                && a.contains("nonce=\"fixture-nonce\"")
                && a.contains("response=\"")
        });
        if !authorized {
            let resp = "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Digest realm=\"fixture\", qop=\"auth\", nonce=\"fixture-nonce\", opaque=\"fixture-opaque\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            stream.write_all(resp.as_bytes()).await?;
            return stream.shutdown().await;
        }
    }

    let (status, content_type, body) = match shared.routes.iter().find(|r| r.matches(&req)) {
        Some(r) => (r.status, r.content_type.clone(), r.body.clone()),
        None => (404, "text/plain".to_string(), b"no fixture route".to_vec()),
    };
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reason(status),
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&body).await?;
    stream.shutdown().await
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Status",
    }
}
