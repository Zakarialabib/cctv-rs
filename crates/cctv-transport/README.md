# cctv-transport

[![Crates.io](https://img.shields.io/crates/v/cctv-transport.svg)](https://crates.io/crates/cctv-transport)
[![docs.rs](https://docs.rs/cctv-transport/badge.svg)](https://docs.rs/cctv-transport)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Network transport, TLS policy management, time-budgeted frame reading, and XML scanning for the `cctv-rs` workspace.

## Core Mechanics

- **`policy::Policy`:** TLS parameters represented purely as data (pinned certs, minimum TLS version, ALPN preference). Easily configured at runtime for self-signed camera certs.
- **`reader::FrameReader`:** Protocol-agnostic, time-budgeted frame buffer preventing slow or stalled network cameras from blocking event loops.
- **`xml` Scanner:** High-performance, total, namespace-prefix-agnostic XML scanner designed specifically for parsing varied IP camera SOAP/REST responses.
- **`net` Feature:** Pure `tokio` + `rustls` socket abstractions (`connect_tcp`, `connect_tls`, `bind_udp`) operating directly on TLS policy without relying on external HTTP client wrappers.
- **`http` Feature:** Shared async HTTP client supporting HTTP Digest and Basic authentication challenge parsing (RFC 2617).

## Usage Example

```rust,no_run
use cctv_transport::net::{connect_tls, TcpOptions};
use cctv_transport::policy::Policy;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut options = TcpOptions::default();
    options.policy = Policy::strict_pinned();
    options.accept_invalid_certs = true; // Opt-in for self-signed camera certs

    let stream = connect_tls("192.168.1.64:443", &options).await?;
    // Read or write directly over secure transport
    Ok(())
}
```

## Technical Guarantees

- `#![forbid(unsafe_code)]`
- `rustls`-only crypto stack (no native-tls or OpenSSL linkages).
- Strict slice boundary verification on all byte chunk parsing.
