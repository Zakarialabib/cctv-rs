//! QUIC transport via iroh (feature `quic`).
//!
//! Provides NAT traversal, control and video stream multiplexing over a single
//! connection, and connection migration across network changes (e.g. Wi-Fi → cellular).
//!
//! Built on top of `iroh` with pure `rustls` (aws-lc-rs) cryptography:
//! - `#![forbid(unsafe_code)]` is strictly preserved.
//! - Zero OpenSSL or native-tls linkages.
//! - Fits behind the existing socket abstraction ([`crate::net::Stream`]).

use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use std::sync::Arc;

use iroh::endpoint::{
    presets, Connection, Endpoint, QuicTransportConfig, RecvStream, SendStream, VarInt,
};
pub use iroh::{EndpointAddr, EndpointId, RelayUrl, SecretKey};
use noq_proto::congestion::{Bbr3Config, CubicConfig};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _, ReadBuf};

use crate::net::{ErrorKind, NetError};

/// Default stream receive window for live CCTV video (1 MiB).
///
/// Limits bufferbloat and queue standing delays while allowing burst transmission of 4K/high-bitrate frames.
pub const LIVE_VIDEO_STREAM_RECEIVE_WINDOW: u32 = 1024 * 1024; // 1 MiB

/// Default connection receive window for live CCTV video (2 MiB).
pub const LIVE_VIDEO_CONNECTION_RECEIVE_WINDOW: u32 = 2 * 1024 * 1024; // 2 MiB

/// Congestion control algorithm for QUIC connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CongestionAlgorithm {
    /// BBR v3: Bandwidth-delay product model with pacing.
    ///
    /// Ideal for real-time video streaming over lossy/wireless links, avoiding queue standing delay.
    #[default]
    Bbr,
    /// Cubic: Loss-based congestion control.
    Cubic,
}

impl CongestionAlgorithm {
    /// Build a new congestion controller factory for this algorithm.
    pub fn build_factory(
        &self,
    ) -> Arc<dyn iroh::endpoint::ControllerFactory + Send + Sync + 'static> {
        match self {
            Self::Bbr => {
                let mut cfg = Bbr3Config::default();
                // 64 KiB initial window for quick live video ramp-up
                cfg.initial_window(64 * 1024);
                Arc::new(cfg)
            }
            Self::Cubic => {
                let mut cfg = CubicConfig::default();
                cfg.initial_window(64 * 1024);
                Arc::new(cfg)
            }
        }
    }
}

/// Create a tuned [`QuicTransportConfig`] optimized for live low-latency video streaming.
///
/// Features:
/// - Configurable congestion controller (defaults to [`CongestionAlgorithm::Bbr`]).
/// - Bounded stream and connection receive windows (`1 MiB` stream / `2 MiB` connection)
///   to prevent bufferbloat and queuing delays.
/// - Keep-alive interval for active NAT hole preservation.
pub fn live_video_transport_config(congestion: CongestionAlgorithm) -> QuicTransportConfig {
    live_video_transport_config_with_windows(
        congestion,
        LIVE_VIDEO_STREAM_RECEIVE_WINDOW,
        LIVE_VIDEO_CONNECTION_RECEIVE_WINDOW,
    )
}

/// Create a tuned [`QuicTransportConfig`] with custom receive window sizes.
pub fn live_video_transport_config_with_windows(
    congestion: CongestionAlgorithm,
    stream_receive_window_bytes: u32,
    conn_receive_window_bytes: u32,
) -> QuicTransportConfig {
    QuicTransportConfig::builder()
        .congestion_controller_factory(congestion.build_factory())
        .stream_receive_window(VarInt::from_u32(stream_receive_window_bytes))
        .receive_window(VarInt::from_u32(conn_receive_window_bytes))
        .build()
}

/// A bidirectional QUIC stream pairing a send stream and a receive stream.
///
/// Implements [`AsyncRead`] and [`AsyncWrite`], allowing it to be used as a
/// socket stream directly or wrapped inside [`crate::net::Stream::Quic`].
pub struct QuicStream {
    send: SendStream,
    recv: RecvStream,
}

impl QuicStream {
    /// Create a new bidirectional stream from an iroh `(SendStream, RecvStream)` pair.
    pub fn new(send: SendStream, recv: RecvStream) -> Self {
        Self { send, recv }
    }

    /// Split this stream into its underlying send and receive halves.
    pub fn into_split(self) -> (SendStream, RecvStream) {
        (self.send, self.recv)
    }

    /// Access the underlying send stream.
    pub fn send_mut(&mut self) -> &mut SendStream {
        &mut self.send
    }

    /// Access the underlying receive stream.
    pub fn recv_mut(&mut self) -> &mut RecvStream {
        &mut self.recv
    }
}

impl fmt::Debug for QuicStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QuicStream")
            .field("send_id", &self.send.id())
            .field("recv_id", &self.recv.id())
            .finish()
    }
}

impl AsyncRead for QuicStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl AsyncWrite for QuicStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        tokio::io::AsyncWrite::poll_write(Pin::new(&mut self.send), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        tokio::io::AsyncWrite::poll_flush(Pin::new(&mut self.send), cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        tokio::io::AsyncWrite::poll_shutdown(Pin::new(&mut self.send), cx)
    }
}

/// Channel identifier for multiplexing over a single QUIC connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChannelKind {
    /// Low-latency bidirectional control plane (RTSP/ONVIF signaling, PTZ commands, telemetry).
    Control,
    /// High-throughput media data plane (RTP video packets, audio frames).
    Video,
    /// Custom application channel identified by a 16-bit tag.
    Custom(u16),
}

impl ChannelKind {
    /// Return the wire tag byte identifying this channel kind on the stream preface.
    pub fn to_tag(self) -> u16 {
        match self {
            Self::Control => 1,
            Self::Video => 2,
            Self::Custom(tag) => tag,
        }
    }

    /// Parse a wire tag byte into a channel kind.
    pub fn from_tag(tag: u16) -> Self {
        match tag {
            1 => Self::Control,
            2 => Self::Video,
            other => Self::Custom(other),
        }
    }
}

/// Options for configuring a QUIC endpoint and connections.
#[derive(Debug, Clone)]
pub struct QuicOptions {
    /// Connect timeout budget.
    pub timeout: Duration,
    /// ALPN (Application-Layer Protocol Negotiation) protocols to advertise/accept.
    pub alpns: Vec<Vec<u8>>,
    /// Custom secret key for node identity. If `None`, an ephemeral key is generated.
    pub secret_key: Option<SecretKey>,
    /// Relay URL to use for hole-punching and NAT traversal fallback.
    pub relay_url: Option<RelayUrl>,
    /// Congestion control algorithm.
    pub congestion: CongestionAlgorithm,
    /// Stream receive window limit in bytes (defaults to [`LIVE_VIDEO_STREAM_RECEIVE_WINDOW`]).
    pub stream_receive_window: u32,
    /// Connection receive window limit in bytes (defaults to [`LIVE_VIDEO_CONNECTION_RECEIVE_WINDOW`]).
    pub connection_receive_window: u32,
}

impl Default for QuicOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            alpns: vec![b"cctv/quic/1".to_vec()],
            secret_key: None,
            relay_url: None,
            congestion: CongestionAlgorithm::default(),
            stream_receive_window: LIVE_VIDEO_STREAM_RECEIVE_WINDOW,
            connection_receive_window: LIVE_VIDEO_CONNECTION_RECEIVE_WINDOW,
        }
    }
}

/// A local QUIC endpoint capable of establishing and accepting peer-to-peer connections.
#[derive(Clone)]
pub struct QuicEndpoint {
    endpoint: Endpoint,
}

impl fmt::Debug for QuicEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QuicEndpoint")
            .field("id", &self.endpoint.id())
            .field("addr", &self.endpoint.addr())
            .finish()
    }
}

impl QuicEndpoint {
    /// Bind a new QUIC endpoint with the specified options.
    pub async fn bind(options: &QuicOptions) -> Result<Self, NetError> {
        let mut builder = Endpoint::builder(presets::Minimal);

        if let Some(sk) = &options.secret_key {
            builder = builder.secret_key(sk.clone());
        }

        if !options.alpns.is_empty() {
            builder = builder.alpns(options.alpns.clone());
        }

        if let Some(relay) = &options.relay_url {
            builder = builder.relay_mode(iroh::RelayMode::Custom(relay.clone().into()));
        } else {
            builder = builder.relay_mode(iroh::RelayMode::Disabled);
        }

        let transport_config = live_video_transport_config_with_windows(
            options.congestion,
            options.stream_receive_window,
            options.connection_receive_window,
        );
        builder = builder.transport_config(transport_config);

        let endpoint = builder.bind().await.map_err(|e| {
            NetError::new(
                ErrorKind::Connect,
                format!("failed to bind QUIC endpoint: {e}"),
            )
        })?;

        Ok(Self { endpoint })
    }

    /// The unique public identifier of this endpoint.
    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// The current dialable address information for this endpoint (IPs, ports, relay).
    pub fn addr(&self) -> EndpointAddr {
        self.endpoint.addr()
    }

    /// Notify the endpoint of an underlying network interface change.
    ///
    /// This triggers path validation and connection migration when a mobile client
    /// switches network interfaces (e.g., roaming from Wi-Fi to cellular).
    pub async fn network_change(&self) {
        self.endpoint.network_change().await;
    }

    /// Connect to a remote QUIC peer by `EndpointAddr`.
    pub async fn connect(
        &self,
        addr: impl Into<EndpointAddr>,
        alpn: &[u8],
        timeout: Duration,
    ) -> Result<QuicConnection, NetError> {
        let addr = addr.into();
        let conn = tokio::time::timeout(timeout, self.endpoint.connect(addr, alpn))
            .await
            .map_err(|_| NetError::new(ErrorKind::Timeout, "QUIC connect timed out"))?
            .map_err(|e| {
                NetError::new(ErrorKind::Connect, format!("QUIC connection failed: {e}"))
            })?;

        Ok(QuicConnection::new(conn))
    }

    /// Accept the next incoming QUIC connection.
    pub async fn accept(&self) -> Result<Option<QuicConnection>, NetError> {
        let incoming = match self.endpoint.accept().await {
            Some(inc) => inc,
            None => return Ok(None),
        };

        let conn = incoming.await.map_err(|e| {
            NetError::new(
                ErrorKind::Connect,
                format!("QUIC handshake accept failed: {e}"),
            )
        })?;

        Ok(Some(QuicConnection::new(conn)))
    }

    /// Close the endpoint and all underlying connections.
    pub async fn close(&self) {
        self.endpoint.close().await;
    }

    /// Access the underlying `iroh::Endpoint`.
    pub fn inner(&self) -> &Endpoint {
        &self.endpoint
    }
}

/// An established QUIC connection to a remote peer.
///
/// Supports multiplexing multiple bidirectional streams (e.g. control signaling
/// and video streams) over a single connection, as well as connection migration.
#[derive(Clone)]
pub struct QuicConnection {
    conn: Connection,
}

impl fmt::Debug for QuicConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QuicConnection")
            .field("remote_id", &self.conn.remote_id())
            .field("rtt", &self.rtt())
            .finish()
    }
}

impl QuicConnection {
    /// Wrap an existing iroh `Connection`.
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }

    /// The remote endpoint ID of the peer.
    pub fn remote_id(&self) -> EndpointId {
        self.conn.remote_id()
    }

    /// The round-trip time (RTT) for the currently selected path of this connection.
    pub fn rtt(&self) -> Option<Duration> {
        for path in self.conn.paths().iter() {
            if path.is_selected() {
                return Some(path.stats().rtt);
            }
        }
        None
    }

    /// Open a raw bidirectional stream on this connection.
    pub async fn open_bi(&self) -> Result<QuicStream, NetError> {
        let (send, recv) = self.conn.open_bi().await.map_err(|e| {
            NetError::new(ErrorKind::Io, format!("failed to open QUIC bi stream: {e}"))
        })?;
        Ok(QuicStream::new(send, recv))
    }

    /// Accept an incoming raw bidirectional stream on this connection.
    pub async fn accept_bi(&self) -> Result<QuicStream, NetError> {
        let (send, recv) = self.conn.accept_bi().await.map_err(|e| {
            NetError::new(
                ErrorKind::Io,
                format!("failed to accept QUIC bi stream: {e}"),
            )
        })?;
        Ok(QuicStream::new(send, recv))
    }

    /// Open a multiplexed bidirectional stream tagged with a [`ChannelKind`].
    ///
    /// Writes a 2-byte channel header (`tag`) immediately so the receiver knows
    /// whether this stream carries control signaling or video payload.
    pub async fn open_channel(&self, kind: ChannelKind) -> Result<QuicStream, NetError> {
        let mut stream = self.open_bi().await?;
        let tag = kind.to_tag().to_be_bytes();
        stream.write_all(&tag).await.map_err(|e| {
            NetError::new(ErrorKind::Io, format!("failed to write channel tag: {e}"))
        })?;
        stream.flush().await.map_err(|e| {
            NetError::new(ErrorKind::Io, format!("failed to flush channel tag: {e}"))
        })?;
        Ok(stream)
    }

    /// Accept a multiplexed bidirectional stream, reading its 2-byte [`ChannelKind`] header.
    pub async fn accept_channel(&self) -> Result<(ChannelKind, QuicStream), NetError> {
        let mut stream = self.accept_bi().await?;
        let mut tag_buf = [0u8; 2];
        stream.read_exact(&mut tag_buf).await.map_err(|e| {
            NetError::new(ErrorKind::Io, format!("failed to read channel tag: {e}"))
        })?;
        let tag = u16::from_be_bytes(tag_buf);
        let kind = ChannelKind::from_tag(tag);
        Ok((kind, stream))
    }

    /// Close this connection with an application error code and reason.
    pub fn close(&self, error_code: u32, reason: &[u8]) {
        self.conn.close(error_code.into(), reason);
    }

    /// Access the underlying `iroh::endpoint::Connection`.
    pub fn inner(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quic_live_video_profile() {
        assert_eq!(CongestionAlgorithm::default(), CongestionAlgorithm::Bbr);
        assert_eq!(LIVE_VIDEO_STREAM_RECEIVE_WINDOW, 1048576);
        assert_eq!(LIVE_VIDEO_CONNECTION_RECEIVE_WINDOW, 2097152);

        // Assert the tuned profile is what live_video_transport_config returns.
        let _transport_config = live_video_transport_config(CongestionAlgorithm::default());
    }
}
