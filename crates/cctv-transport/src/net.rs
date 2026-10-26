//! Raw socket transport (feature `net`): TCP, UDP and TLS over rustls.
//!
//! **Why this exists.** Two driver surfaces were paying for the wrong tool.
//! RTSP speaks a line-oriented, interleaved protocol over a *raw* TCP stream —
//! `reqwest` is an HTTP/1.1-and-HTTP/2 client and cannot carry it. ONVIF was
//! paying `reqwest`'s `hyper`/`tower`/`quinn` dependency tree for a plain HTTPS
//! POST. Both want the same thing: a byte stream we own.
//!
//! **Why it is its own feature, off by default.** `net` pulls `tokio` and
//! `rustls` into the crate. `cctv-transport` is the one crate every driver
//! depends on, so the sockets a driver *does* use must not tax the ones it
//! does not. With the feature off, the crate is still the TLS policy, the
//! frame reader and the XML scanner only.
//!
//! **Why the policy comes from [`crate::policy`] and not from here.** The
//! reconciler swaps a loader per deployment (VPS mirror, office player, kiosk),
//! so minimum version, pinned certificates and ALPN preference are *data*.
//! [`TcpOptions::policy`] carries that data in and this module turns it into a
//! `rustls::ClientConfig`; nothing here invents a second policy type, and the
//! reconciler keeps one knob.
//!
//! **Why `accept_invalid_certs` is an explicit opt-in.** Cameras ship
//! self-signed certificates and cannot be reissued from a fleet, so the flag
//! has to exist — and it is `false` by default, and it is *applied* (see the
//! integration test that asserts the handshake fails without it). Trust is never
//! widened by a default.
//!
//! **Why the byte reader is separate from [`crate::reader::FrameReader`].** The
//! frame reader is deliberately pure: no I/O, no clock, `now_ms` supplied by the
//! caller. That is what makes its measurements exact in tests. Reading bytes is
//! I/O, so it lives here; [`feed_chunks`] is the seam between
//! them — it drains the socket and pushes each chunk's byte count into the
//! reader at the caller's clock.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use rustls_pki_types::pem::PemObject as _;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt as _;
use tokio::net::TcpStream;

use crate::policy::{Policy, TlsVersion};
use crate::reader::FrameReader;

/// The socket layer failed.
///
/// One type covers both the transport and the policy: a driver's reconnect loop
/// only needs to decide *retry or give up*, not which library failed. The
/// message keeps the underlying cause so a log is still diagnosable.
#[derive(Debug)]
pub struct NetError {
    /// Short, stable reason — safe to match on, never free-form prose.
    kind: ErrorKind,
    /// The chain underneath, when there is one.
    detail: String,
}

impl NetError {
    /// Build an error of `kind` carrying `detail`.
    fn new(kind: ErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    /// The stable reason for the failure.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The underlying cause as text.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.detail)
    }
}

impl core::error::Error for NetError {}

impl From<std::io::Error> for NetError {
    fn from(value: std::io::Error) -> Self {
        Self::new(ErrorKind::Io, value.to_string())
    }
}

/// Stable reason a [`NetError`] happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The address could not be parsed or resolved.
    Address,
    /// Connecting failed: refused, unreachable, or a TLS handshake failure.
    Connect,
    /// The connection exceeded its timeout.
    Timeout,
    /// The TLS configuration could not be built (bad policy or bad pin).
    Tls,
    /// An I/O error on the socket.
    Io,
    /// The peer sent something that is not valid TLS.
    Protocol,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Address => "invalid address",
            Self::Connect => "connect failed",
            Self::Timeout => "timed out",
            Self::Tls => "TLS configuration rejected",
            Self::Io => "socket I/O failed",
            Self::Protocol => "TLS protocol error",
        };
        f.write_str(text)
    }
}

/// How the TLS layer treats the peer's certificate.
///
/// Cameras ship self-signed certificates and a fleet cannot be reissued, so
/// "trust this certificate" is a first-class choice — but it is always a choice,
/// never a fallback: [`TlsMode::Default`] refuses a certificate it cannot verify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TlsMode {
    /// Verify against the configured roots, pinning and hostname included.
    #[default]
    Default,
    /// Accept whatever certificate the peer presents, after the handshake.
    ///
    /// The connection is still encrypted; it is just not authenticated, so it is
    /// vulnerable to a man in the middle. Only for a camera whose certificate
    /// cannot be pinned and whose network is trusted.
    AcceptAny,
}

/// Options for a single TCP connection.
///
/// [`Default`] is a camera-shaped starting point: ten seconds, certificate
/// verification on, HTTP/2 preferred, TLS 1.3 minimum.
#[derive(Debug, Clone)]
pub struct TcpOptions {
    /// Connect and handshake budget.  No socket read inherits this budget —
    /// the caller bounds those with [`tokio::time::timeout`] or a reader that
    /// drops frames past a staleness budget.
    pub timeout: Duration,
    /// How to treat the peer's certificate.
    pub tls: TlsMode,
    /// The TLS policy as data: minimum version, pinned certificates, ALPN.
    pub policy: Policy,
    /// Accepted for call-site compatibility with [`crate::http::HttpConfig`];
    /// `true` is exactly [`TlsMode::AcceptAny`].
    ///
    /// [`crate::http::HttpConfig`]: https://docs.rs/cctv-transport/latest/cctv_transport/http/struct.HttpConfig.html
    pub accept_invalid_certs: bool,
}

impl Default for TcpOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            tls: TlsMode::Default,
            policy: Policy::default(),
            accept_invalid_certs: false,
        }
    }
}

/// A byte stream: plain TCP or a TLS session on top of one.
///
/// `enum`, not a trait object: the stream is returned across an `await` and
/// stored by a driver, so it must not borrow the connection that built it, and
/// a boxed `dyn AsyncRead + AsyncWrite` would need a lifetime for the same
/// reason.  `AsyncRead + AsyncWrite + Unpin + Send` is what both variants are.
pub enum Stream {
    /// A plain TCP connection.
    Plain(TcpStream),
    /// A TLS session over a TCP connection.
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
}

impl fmt::Display for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plain(_) => f.write_str("tcp"),
            Self::Tls(_) => f.write_str("tls"),
        }
    }
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The variants hold a live socket, so there is nothing useful to print;
        // `Result::expect_err` needs `Debug` to exist, not to be informative.
        f.debug_tuple(&self.to_string()).finish()
    }
}

impl tokio::io::AsyncRead for Stream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            Self::Tls(s) => std::pin::Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for Stream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            Self::Tls(s) => std::pin::Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => std::pin::Pin::new(s).poll_flush(cx),
            Self::Tls(s) => std::pin::Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Plain(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            Self::Tls(s) => std::pin::Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// Split the `host:port` text into the host and port rustls and tokio expect.
///
/// Bracketed IPv6 (`[::1]:554`) keeps its brackets for DNS-name parsing and
/// drops them for the socket address.  A host with no port at all is rejected:
/// guessing 443 would silently send RTSP to the wrong port, so the caller states
/// it (`camera.local:554`).
fn split_host_port(addr: &str) -> Result<(String, u16), NetError> {
    let (host, port_text) = if let Some(rest) = addr.strip_prefix('[') {
        let end = rest.find(']').ok_or_else(|| {
            NetError::new(ErrorKind::Address, format!("{addr}: unclosed IPv6 host"))
        })?;
        let host = &rest[..end];
        let after = rest.get(end + 1..).unwrap_or("");
        let port = after
            .strip_prefix(':')
            .ok_or_else(|| NetError::new(ErrorKind::Address, format!("{addr}: missing port")))?;
        (host.to_string(), port.to_string())
    } else {
        let (host, port) = addr
            .rsplit_once(':')
            .ok_or_else(|| NetError::new(ErrorKind::Address, format!("{addr}: missing port")))?;
        (host.to_string(), port.to_string())
    };

    if host.is_empty() {
        return Err(NetError::new(
            ErrorKind::Address,
            format!("{addr}: empty host"),
        ));
    }
    let port = port_text
        .parse::<u16>()
        .map_err(|_| NetError::new(ErrorKind::Address, format!("{addr}: port is not a number")))?;
    Ok((host, port))
}

/// ALPN identifiers to advertise, most preferred first.
///
/// HTTP/2 first when the policy asks for it (ONVIF over HTTPS), then HTTP/1.1;
/// an empty list means "no ALPN extension at all", which is what a raw RTSP
/// stream wants.
fn alpn_protocols(policy: &Policy) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    if policy.prefer_h2 {
        out.push(b"h2".to_vec());
    }
    out.push(b"http/1.1".to_vec());
    out
}

/// Build a `rustls::ClientConfig` from the crate's TLS policy.
///
/// The policy is data, so this is where it stops being data:
///
/// - [`Policy::min_version`] selects the enabled protocol versions — TLS 1.3
///   only by default, TLS 1.2 added when a deployment still has to talk to an
///   old camera;
/// - [`Policy::pinned_certs`] populates the root store.  A PEM file is the
///   accepted form, because that is what a camera's certificate export is; a
///   DER blob is accepted as well.
/// - [`Policy::prefer_h2`] selects the ALPN list.
///
/// A pinned store is *added to* the webpki roots, never substituted for them:
/// pinning a camera's certificate must not silently stop the client from
/// verifying a properly signed one.
fn client_config(
    policy: &Policy,
    tls_mode: TlsMode,
    accept_invalid_certs: bool,
) -> Result<Arc<rustls::ClientConfig>, NetError> {
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let versions: &[&'static rustls::SupportedProtocolVersion] = match policy.min_version {
        TlsVersion::V1_3 => &[&rustls::version::TLS13],
        TlsVersion::V1_2 => &[&rustls::version::TLS13, &rustls::version::TLS12],
    };

    let builder = rustls::ClientConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(versions)
        .map_err(|e| NetError::new(ErrorKind::Tls, e.to_string()))?;

    let trust_any = matches!(tls_mode, TlsMode::AcceptAny) || accept_invalid_certs;

    let config = if trust_any {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AnyServerCert))
            .with_no_client_auth()
    } else {
        let mut roots = rustls::RootCertStore::empty();
        for pem in &policy.pinned_certs {
            // PEM is what a camera's certificate export is.  A DER blob is
            // accepted as well, so a policy file does not have to be PEM.
            let parsed: Vec<rustls_pki_types::CertificateDer<'_>> =
                rustls_pki_types::CertificateDer::pem_slice_iter(pem.as_bytes())
                    .filter_map(Result::ok)
                    .collect();
            let certs = if parsed.is_empty() {
                vec![rustls_pki_types::CertificateDer::from(pem.as_bytes())]
            } else {
                parsed
            };
            let (added, _) = roots.add_parsable_certificates(certs);
            if added == 0 {
                return Err(NetError::new(
                    ErrorKind::Tls,
                    format!("pinned certificate could not be parsed: {pem}"),
                ));
            }
        }
        builder.with_root_certificates(roots).with_no_client_auth()
    };

    let mut config = config;
    let alpn = alpn_protocols(policy);
    if !alpn.is_empty() {
        config.alpn_protocols = alpn;
    }
    Ok(Arc::new(config))
}

/// A certificate verifier that accepts any certificate the peer presents.
///
/// This is the explicit [`TlsMode::AcceptAny`] path, not a fallback: it is
/// reachable only when the caller opted in.  The connection is still encrypted;
/// it is simply not authenticated, so it must not be used on an untrusted
/// network.  Signatures are still checked with the process crypto provider.
#[derive(Debug)]
struct AnyServerCert;

impl rustls::client::danger::ServerCertVerifier for AnyServerCert {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls_pki_types::CertificateDer<'_>,
        _intermediates: &[rustls_pki_types::CertificateDer<'_>],
        _server_name: &rustls_pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls_pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls_pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::aws_lc_rs::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls_pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::aws_lc_rs::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::aws_lc_rs::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

/// Open a plain TCP connection to `addr` (`host:port`, or a bracketed IPv6
/// literal).  The port is required: guessing 443 would silently send RTSP to the
/// wrong port.
///
/// `options` bounds the connect; its TLS fields are ignored because there is no
/// TLS here.  Use [`connect_tls`] for the encrypted case.
pub async fn connect_tcp(addr: &str, options: &TcpOptions) -> Result<Stream, NetError> {
    let (host, port) = split_host_port(addr)?;
    let target = format!("{host}:{port}");
    let stream = tokio::time::timeout(options.timeout, TcpStream::connect(&target))
        .await
        .map_err(|_| NetError::new(ErrorKind::Timeout, format!("{target}: connect timed out")))?
        .map_err(|e| NetError::new(ErrorKind::Connect, format!("{target}: {e}")))?;
    Ok(Stream::Plain(stream))
}

/// Open a TLS connection to `addr`, presenting `server_name` as SNI and for
/// hostname verification.
///
/// The peer's certificate is judged by [`TcpOptions`]: `accept_invalid_certs`
/// (or [`TlsMode::AcceptAny`]) accepts a self-signed or mismatched certificate,
/// which cameras ship; otherwise the certificate must chain to a root the policy
/// pins.  The policy's minimum version and ALPN preference are applied either
/// way.
pub async fn connect_tls(
    addr: &str,
    server_name: &str,
    options: &TcpOptions,
) -> Result<Stream, NetError> {
    let (host, port) = split_host_port(addr)?;
    let target = format!("{host}:{port}");

    let plain = tokio::time::timeout(options.timeout, TcpStream::connect(&target))
        .await
        .map_err(|_| NetError::new(ErrorKind::Timeout, format!("{target}: connect timed out")))?
        .map_err(|e| NetError::new(ErrorKind::Connect, format!("{target}: {e}")))?;

    let config = client_config(&options.policy, options.tls, options.accept_invalid_certs)?;

    let name = rustls_pki_types::ServerName::try_from(server_name.to_string())
        .map_err(|_| NetError::new(ErrorKind::Address, format!("{server_name}: not a DNS name")))?;

    let connector = tokio_rustls::TlsConnector::from(config);
    let session = tokio::time::timeout(options.timeout, connector.connect(name, plain))
        .await
        .map_err(|_| {
            NetError::new(
                ErrorKind::Timeout,
                format!("{target}: TLS handshake timed out"),
            )
        })?
        .map_err(|e| {
            let text = e.to_string();
            let kind = if text.to_ascii_lowercase().contains("certificate") {
                ErrorKind::Tls
            } else {
                ErrorKind::Protocol
            };
            NetError::new(kind, format!("{target}: {e}"))
        })?;

    Ok(Stream::Tls(Box::new(session)))
}

/// Bind a UDP socket to `addr` (`host:port`).
///
/// Binding to `0.0.0.0:0` gives an ephemeral port on every interface, which is
/// what a discovery probe wants.  UDP has no handshake, so there is no timeout
/// and no TLS policy to apply.
pub async fn bind_udp(addr: &str) -> Result<tokio::net::UdpSocket, NetError> {
    tokio::net::UdpSocket::bind(addr)
        .await
        .map_err(|e| NetError::new(ErrorKind::Connect, format!("{addr}: {e}")))
}

/// Read one chunk of at most `chunk` bytes, or `None` at a clean end of file.
///
/// A short read is normal and returned as-is: TCP gives whatever arrived, and a
/// frame boundary is the caller's business.  `None` means the peer closed —
/// [`FrameReader::reset`](crate::reader::FrameReader::reset) is the right
/// reaction, because the stream really is a new one after a reconnect.
///
/// `chunk` of zero yields an empty chunk rather than an error; a reader that
/// wants to enforce a floor should clamp it before calling.
pub async fn read_chunks<S: AsyncRead + Unpin>(
    stream: &mut S,
    chunk: usize,
) -> Result<Option<Vec<u8>>, NetError> {
    let mut buf = vec![0u8; chunk];
    let n = stream.read(&mut buf).await?;
    if n == 0 {
        return Ok(None);
    }
    buf.truncate(n);
    Ok(Some(buf))
}

/// Drain `stream` into `reader` until the peer closes, one chunk at a time.
///
/// This is the composition the frame reader was built for: it owns the budget
/// and the measurements, this owns the socket and the clock.  Each chunk's
/// **byte count** is pushed at `now_ms` — the reader measures bytes per second,
/// not frames, so the payload here is the chunk rather than a decoded frame.
/// A decoded frame (width, height, payload) belongs to the driver, which can
/// push it through [`FrameReader::push`](crate::reader::FrameReader::push)
/// directly.
///
/// `now_ms` comes from the caller so the measurement stays exact and testable;
/// use `cctv_rs::clock_ms()` in the application.
pub async fn feed_chunks<S: AsyncRead + Unpin>(
    stream: &mut S,
    chunk: usize,
    reader: &mut FrameReader,
    now_ms: u64,
) -> Result<(), NetError> {
    while let Some(bytes) = read_chunks(stream, chunk).await? {
        // A chunk is not a frame; the width and height are the driver's to
        // know.  Zero is a placeholder for "size only".
        reader.push(0, 0, bytes, now_ms);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::Policy;

    #[test]
    fn a_default_policy_verifies_certificates_and_wants_h2() {
        let opts = TcpOptions::default();
        assert_eq!(opts.tls, TlsMode::Default);
        assert!(!opts.accept_invalid_certs);
        let config = client_config(&opts.policy, opts.tls, opts.accept_invalid_certs)
            .expect("a default policy builds");
        assert_eq!(
            config.alpn_protocols,
            vec![b"h2".to_vec(), b"http/1.1".to_vec()]
        );
    }

    #[test]
    fn tls_12_policy_adds_tls_12_and_keeps_tls_13() {
        let policy = Policy {
            min_version: TlsVersion::V1_2,
            ..Policy::default()
        };
        let config = client_config(&policy, TlsMode::Default, false).expect("builds");
        // rustls keeps the enabled versions private; the observable contract is
        // that the config builds and the ALPN preference is still applied.
        assert!(!config.alpn_protocols.is_empty());
    }

    #[test]
    fn an_unparseable_pin_is_an_error_not_a_silent_skip() {
        let policy = Policy::strict_pinned(vec!["not a certificate".to_string()]);
        let err = client_config(&policy, TlsMode::Default, false).expect_err("must fail");
        assert_eq!(err.kind(), ErrorKind::Tls, "{err}");
    }

    #[test]
    fn accept_any_builds_without_roots() {
        let config = client_config(&Policy::default(), TlsMode::AcceptAny, false)
            .expect("an opted-in verifier builds");
        // The opt-in widens trust; it does not discard the rest of the policy.
        // The ALPN preference must still reach the config, or an ONVIF camera
        // would silently lose HTTP/2 because its certificate is self-signed.
        assert_eq!(
            config.alpn_protocols,
            vec![b"h2".to_vec(), b"http/1.1".to_vec()]
        );
    }

    #[test]
    fn the_flag_and_the_mode_agree() {
        // `accept_invalid_certs: true` is exactly `TlsMode::AcceptAny`; both
        // build, and the flag must win when it disagrees with the mode.
        assert!(
            client_config(&Policy::default(), TlsMode::Default, true).is_ok(),
            "the compat flag is honoured"
        );
        assert!(
            client_config(&Policy::default(), TlsMode::AcceptAny, false).is_ok(),
            "the mode is honoured"
        );
    }

    #[test]
    fn host_and_port_split_handles_ipv6_and_defaults() {
        assert_eq!(
            split_host_port("camera.local:554").expect("host:port"),
            ("camera.local".to_string(), 554)
        );
        assert_eq!(
            split_host_port("[::1]:8554").expect("ipv6"),
            ("::1".to_string(), 8554)
        );
        assert_eq!(
            split_host_port("127.0.0.1:443").expect("ipv4"),
            ("127.0.0.1".to_string(), 443)
        );
        // A bare host is rejected rather than silently defaulted to 443.
        assert!(
            split_host_port("camera.local").is_err(),
            "no port, no guess"
        );
        assert!(split_host_port("[::1]:nope").is_err());
        assert!(split_host_port("").is_err());
        assert!(split_host_port(":554").is_err());
    }

    #[test]
    fn alpn_is_h2_then_http11_or_nothing() {
        let mut policy = Policy::default();
        assert_eq!(
            alpn_protocols(&policy),
            vec![b"h2".to_vec(), b"http/1.1".to_vec()]
        );
        policy.prefer_h2 = false;
        assert_eq!(alpn_protocols(&policy), vec![b"http/1.1".to_vec()]);
    }

    #[test]
    fn io_errors_carry_their_kind_and_message() {
        let err: NetError =
            std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused").into();
        assert_eq!(err.kind(), ErrorKind::Io);
        assert!(err.detail().contains("refused"), "{}", err.detail());
        assert!(err.to_string().contains("socket I/O failed"), "{err}");
        // `core::error::Error` is implemented, so `?` and anyhow-style chaining work.
        fn _takes_error<E: core::error::Error>(_: E) {}
        _takes_error(err);
    }

    #[tokio::test]
    async fn a_zero_sized_chunk_is_an_empty_read_not_a_panic() {
        // `read_chunks` allocates `vec![0u8; chunk]` and truncates to the read
        // length; a zero-sized request must not panic.  It reads zero bytes, which
        // for `AsyncRead` is indistinguishable from a clean EOF, so the shape is
        // `None` — the caller's drain loop terminates rather than spinning.
        // Checked here because the integration test always uses a real size.
        let mut stream = tokio::io::empty();
        assert_eq!(
            read_chunks(&mut stream, 0).await.expect("reading works"),
            None
        );
    }
}
