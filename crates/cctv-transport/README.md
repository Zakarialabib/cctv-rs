# cctv-transport

[![Crates.io](https://img.shields.io/crates/v/cctv-transport.svg)](https://crates.io/crates/cctv-transport)
[![docs.rs](https://docs.rs/cctv-transport/badge.svg)](https://docs.rs/cctv-transport)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Transport foundation for `cctv-rs`: data-driven TLS policy, time-budgeted frame reading, zero-allocation XML scanning, and pure async sockets.

## Primitives

- **Data-Driven TLS (`policy::Policy`)**: TLS config as data (pins, versions, ALPN). Easily configured for self-signed camera certificates.
- **Budgeted Reader (`reader::FrameReader`)**: Protocol-agnostic, time-budgeted ring buffer preventing stalled camera streams from blocking event loops.
- **Zero-Alloc XML (`xml`)**: High-performance, prefix-agnostic XML scanner designed for varied IP camera SOAP/REST responses.
- **Pure Sockets (Feature `net`)**: `tokio` + `rustls` TCP/TLS socket abstractions (`connect_tcp`, `connect_tls`, `bind_udp`) operating directly on TLS policies.
- **HTTP Client (Feature `http`)**: Async HTTP client supporting HTTP Digest and Basic authentication challenge parsing (RFC 2617).

## Invariants

- `#![forbid(unsafe_code)]`
- Strict `rustls`-only crypto stack (zero OpenSSL / native-tls linkages).

## Usage

```rust,no_run
use cctv_transport::net::{connect_tls, TcpOptions};
use cctv_transport::policy::Policy;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = TcpOptions::default();
    options.policy = Policy::strict_pinned();
    options.accept_invalid_certs = true;

    let _stream = connect_tls("192.168.1.64:443", &options).await?;
    Ok(())
}
```
