//! The socket layer against real sockets (feature `net`).
//!
//! Nothing here talks to a camera.  The plain-TCP and UDP cases use a listener
//! and a socket the test itself owns; the TLS case starts a server with a
//! self-signed certificate generated at test time by `rcgen`, so no key material
//! is checked in and — more importantly — the certificate is *genuinely*
//! unverifiable rather than a canned one the test could accidentally trust.
//!
//! The assertion that matters is the TLS opt-in pair: the same server, the same
//! address, and the handshake succeeds with `accept_invalid_certs: true` and
//! fails with it `false`.  That proves the flag is wired to the verifier, not
//! merely accepted.

#![cfg(feature = "net")]

use std::sync::Arc;
use std::time::Duration;

use cctv_transport::net::{self, ErrorKind, TcpOptions, TlsMode};
use cctv_transport::policy::{Policy, TlsVersion};
use cctv_transport::reader::FrameReader;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::{TcpListener, UdpSocket};

/// A server that echoes one line per connection, then closes.
///
/// The plain-TCP test needs a peer that proves the bytes went somewhere and came
/// back — an echo does that without the canned HTTP routes of the `fixture`
/// feature, which would drag that feature (and its feature-gate coupling) into a
/// test about sockets.
async fn start_echo() -> std::io::Result<std::net::SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                loop {
                    let n = match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    if stream.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    Ok(addr)
}

#[tokio::test]
async fn plain_tcp_connects_and_round_trips_bytes() {
    let addr = start_echo().await.expect("echo server binds");
    let options = TcpOptions::default();

    let mut stream = net::connect_tcp(&addr.to_string(), &options)
        .await
        .expect("a loopback connect succeeds");

    let payload = b"OPTIONS rtsp://camera/ RTSP/1.0\r\n";
    stream.write_all(payload).await.expect("write succeeds");
    stream.flush().await.expect("flush succeeds");

    let mut echoed = vec![0u8; payload.len()];
    stream
        .read_exact(&mut echoed)
        .await
        .expect("the echo comes back");
    assert_eq!(echoed, payload, "the bytes survive the round trip");
}

#[tokio::test]
async fn a_refused_port_is_a_connect_error_not_a_panic() {
    // A listening socket dropped without a close: the port is bound, then gone,
    // so a connect to it is refused rather than answered.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
    let addr = listener
        .local_addr()
        .expect("a bound socket has an address");
    drop(listener);

    let options = TcpOptions::default();
    let err = net::connect_tcp(&addr.to_string(), &options)
        .await
        .expect_err("nothing is listening any more");
    assert_eq!(err.kind(), ErrorKind::Connect, "{err}");
    assert!(
        !err.detail().is_empty(),
        "the cause is kept: {}",
        err.detail()
    );
}

#[tokio::test]
async fn a_connect_past_the_timeout_reports_timeout() {
    let options = TcpOptions {
        // Blackholed TEST-NET-1: connect cannot complete, it can only hang.
        timeout: Duration::from_millis(200),
        ..TcpOptions::default()
    };
    let err = net::connect_tcp("192.0.2.1:8554", &options)
        .await
        .expect_err("a blackholed address cannot connect");
    assert_eq!(err.kind(), ErrorKind::Timeout, "{err}");
}

#[tokio::test]
async fn udp_binds_sends_and_receives_on_loopback() {
    let server = net::bind_udp("127.0.0.1:0")
        .await
        .expect("a loopback udp bind succeeds");
    let client = UdpSocket::bind("127.0.0.1:0")
        .await
        .expect("the client binds too");
    let server_addr = server.local_addr().expect("a bound socket has an address");

    let probe = b"discovery probe";
    client
        .send_to(probe, server_addr)
        .await
        .expect("send_to succeeds");

    let mut buf = [0u8; 64];
    let (n, from) = server.recv_from(&mut buf).await.expect("the probe arrives");
    assert_eq!(&buf[..n], probe, "the datagram survives");
    assert_eq!(from, client.local_addr().expect("client address"));
}

/// A TLS server that completes a handshake and echoes one buffer.
struct TlsEcho {
    addr: std::net::SocketAddr,
    /// The PEM of the self-signed certificate it serves, so a test can pin it.
    cert_pem: String,
    _task: tokio::task::JoinHandle<()>,
}

async fn start_tls_echo() -> TlsEcho {
    // Generated at test time: no checked-in key, and the certificate really is
    // self-signed, so a verifying client has no reason to accept it.
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
        .expect("a self-signed certificate generates");
    let cert_pem = certified.cert.pem();
    let cert_der = rustls_pki_types::CertificateDer::from(certified.cert.der().to_vec());
    let key_der =
        rustls_pki_types::PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der()).into();

    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .expect("the generated key pair matches its certificate");

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("the tls server binds");
    let addr = listener.local_addr().expect("bound");
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));

    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(stream).await else {
                    return;
                };
                let mut buf = [0u8; 1024];
                let Ok(n) = tls.read(&mut buf).await else {
                    return;
                };
                if n > 0 {
                    let _ = tls.write_all(&buf[..n]).await;
                    let _ = tls.shutdown().await;
                }
            });
        }
    });

    TlsEcho {
        addr,
        cert_pem,
        _task: task,
    }
}

#[tokio::test]
async fn tls_handshake_succeeds_when_the_self_signed_cert_is_accepted() {
    let server = start_tls_echo().await;
    let options = TcpOptions {
        accept_invalid_certs: true,
        ..TcpOptions::default()
    };

    let mut stream = net::connect_tls(&server.addr.to_string(), "localhost", &options)
        .await
        .expect("the opt-in reaches the verifier");

    stream
        .write_all(b"hi")
        .await
        .expect("the session carries bytes");
    let mut echoed = [0u8; 2];
    stream
        .read_exact(&mut echoed)
        .await
        .expect("the echo comes back over TLS");
    assert_eq!(&echoed, b"hi");
}

#[tokio::test]
async fn tls_handshake_fails_without_the_opt_in() {
    let server = start_tls_echo().await;
    let options = TcpOptions::default();
    assert!(!options.accept_invalid_certs, "the default is off");

    let err = net::connect_tls(&server.addr.to_string(), "localhost", &options)
        .await
        .expect_err("a self-signed certificate must not verify");
    assert_eq!(err.kind(), ErrorKind::Tls, "{err}");
    assert!(
        err.detail().to_ascii_lowercase().contains("certificate"),
        "the cause names the certificate: {}",
        err.detail()
    );
}

#[tokio::test]
async fn the_flag_and_the_mode_are_the_same_door() {
    // `TlsMode::AcceptAny` is the explicit spelling of `accept_invalid_certs`;
    // both must open the session, so a caller can pick either.
    let server = start_tls_echo().await;

    let by_mode = TcpOptions {
        tls: TlsMode::AcceptAny,
        ..TcpOptions::default()
    };
    net::connect_tls(&server.addr.to_string(), "localhost", &by_mode)
        .await
        .expect("the mode is honoured");

    let by_flag = TcpOptions {
        accept_invalid_certs: true,
        ..TcpOptions::default()
    };
    net::connect_tls(&server.addr.to_string(), "localhost", &by_flag)
        .await
        .expect("the flag is honoured");
}

#[tokio::test]
async fn a_minimum_version_of_tls_12_is_applied_and_still_connects() {
    let server = start_tls_echo().await;
    let options = TcpOptions {
        accept_invalid_certs: true,
        policy: Policy {
            min_version: TlsVersion::V1_2,
            ..Policy::default()
        },
        ..TcpOptions::default()
    };
    let mut stream = net::connect_tls(&server.addr.to_string(), "localhost", &options)
        .await
        .expect("tls 1.2 minimum still negotiates a session");

    stream.write_all(b"v").await.expect("the session works");
    let mut echoed = [0u8; 1];
    stream.read_exact(&mut echoed).await.expect("echo");
    assert_eq!(&echoed, b"v");
}

#[tokio::test]
async fn a_pinned_certificate_is_accepted_without_the_invalid_cert_opt_in() {
    // The other half of the trust story: a camera's self-signed certificate can
    // be *pinned* instead of bypassing verification entirely, and that must work
    // with `accept_invalid_certs: false`. Without it a fleet whose certificates
    // cannot be reissued has no trusted option at all — only "trust nothing".
    let server = start_tls_echo().await;
    let options = TcpOptions {
        accept_invalid_certs: false,
        policy: Policy::strict_pinned(vec![server.cert_pem.clone()]),
        ..TcpOptions::default()
    };

    let mut stream = net::connect_tls(&server.addr.to_string(), "localhost", &options)
        .await
        .expect("a pinned camera certificate verifies without the opt-in");

    stream.write_all(b"p").await.expect("the session works");
    let mut echoed = [0u8; 1];
    stream.read_exact(&mut echoed).await.expect("echo");
    assert_eq!(&echoed, b"p");

    // The pin is a real pin: a *different* self-signed certificate is refused
    // even though it was generated the same way and carries the same name.
    let stranger = start_tls_echo().await;
    let stranger_options = TcpOptions {
        accept_invalid_certs: false,
        policy: Policy::strict_pinned(vec![server.cert_pem.clone()]),
        ..TcpOptions::default()
    };
    assert!(
        net::connect_tls(&stranger.addr.to_string(), "localhost", &stranger_options)
            .await
            .is_err(),
        "a certificate outside the pin is refused"
    );
}

#[tokio::test]
async fn read_chunks_returns_none_on_clean_eof_and_a_chunk_on_data() {
    // `tokio::io::duplex` is an in-memory pipe: one end is the "server", the
    // other is what `read_chunks` reads, and dropping the server end is a clean
    // EOF.  No listener, no port, no race.
    let (mut server, mut client) = tokio::io::duplex(64);

    // A chunk on data: a short read is returned as-is, not padded to `chunk`.
    server.write_all(b"abc").await.expect("the server writes");
    server.flush().await.expect("flushed");
    let chunk = net::read_chunks(&mut client, 16)
        .await
        .expect("reading works")
        .expect("data is not eof");
    assert_eq!(chunk, b"abc");

    // Drop the writer: the peer closed, so the next read is a clean EOF.
    drop(server);
    assert_eq!(
        net::read_chunks(&mut client, 16)
            .await
            .expect("reading works"),
        None,
        "a closed peer is `None`, not an error"
    );
    // And it stays `None` — a caller's drain loop terminates.
    assert_eq!(
        net::read_chunks(&mut client, 16).await.expect("works"),
        None
    );
}

#[tokio::test]
async fn feed_chunks_pushes_byte_counts_into_the_frame_reader() {
    // `duplex` gives a deterministic peer that writes a known payload and closes,
    // so the drain loop terminates without a socket race and without a timeout.
    let (mut server, mut stream) = tokio::io::duplex(1 << 16);
    let payload = vec![7u8; 5_000];
    server.write_all(&payload).await.expect("the server writes");
    drop(server);

    // The reader is pure and takes `now_ms`; this reader is I/O, so the clock is
    // the caller's — the composition the module documents.
    let mut reader = FrameReader::new();
    net::feed_chunks(&mut stream, 1_024, &mut reader, 2_000)
        .await
        .expect("the drain completes");

    // `FrameReader` clamps its window to at least 1 s, so 2 s is the shortest
    // window that holds the whole drain without the rate being scaled up.
    assert_eq!(
        reader.bytes_per_sec(2_000) as usize * 2,
        payload.len(),
        "every byte the peer sent reached the reader's measurement"
    );
    assert!(reader.received() > 0, "the reader saw the chunks");
}

#[tokio::test]
async fn a_malformed_address_is_an_address_error() {
    let err = net::connect_tcp("no-port-here", &TcpOptions::default())
        .await
        .expect_err("an address without a port is rejected");
    assert_eq!(err.kind(), ErrorKind::Address, "{err}");

    let err = net::bind_udp("not-an-address")
        .await
        .expect_err("a malformed udp address is rejected");
    assert_eq!(err.kind(), ErrorKind::Connect, "{err}");
}
