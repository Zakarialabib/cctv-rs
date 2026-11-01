# cctv-rs

[![Crates.io](https://img.shields.io/crates/v/cctv-rs.svg)](https://crates.io/crates/cctv-rs)
[![docs.rs](https://docs.rs/cctv-rs/badge.svg)](https://docs.rs/cctv-rs)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Build Status](https://img.shields.io/github/actions/workflow/status/Zakarialabib/cctv-rs/ci.yml?branch=main)](https://github.com/Zakarialabib/cctv-rs/actions)

A modular, zero-panic, pure-Rust CCTV backend engine. Discover, stream, control, and reconcile IP camera fleets across ONVIF, Ezviz, RTSP, and custom protocols behind a single typed facade.

No C bindings, no vendor SDK bloat, no UI, no database dependencies, and `#![forbid(unsafe_code)]` across every crate.

## The Architecture

`cctv-rs` breaks camera handling into decoupled, single-responsibility driver crates unified by a zero-runtime core contract (`cctv-core`).

| Crate | Crates.io | Docs.rs | Description |
| --- | --- | --- | --- |
| [`cctv-core`](crates/cctv-core) | [![v](https://img.shields.io/crates/v/cctv-core.svg)](https://crates.io/crates/cctv-core) | [![docs](https://docs.rs/cctv-core/badge.svg)](https://docs.rs/cctv-core) | `alloc`-only device model: typed commands, error taxonomy, state machine & mock suite. Zero Tokio, zero async runtime. |
| [`cctv-transport`](crates/cctv-transport) | [![v](https://img.shields.io/crates/v/cctv-transport.svg)](https://crates.io/crates/cctv-transport) | [![docs](https://docs.rs/cctv-transport/badge.svg)](https://docs.rs/cctv-transport) | Time-budgeted `FrameReader`, zero-allocation XML scanner, and pure TCP/TLS socket abstractions (rustls only). |
| [`cctv-rtp`](crates/cctv-rtp) | [![v](https://img.shields.io/crates/v/cctv-rtp.svg)](https://crates.io/crates/cctv-rtp) | [![docs](https://docs.rs/cctv-rtp/badge.svg)](https://docs.rs/cctv-rtp) | Zero-dependency RTP header parsing and H.264/H.265 access-unit depacketization (STAP-A, FU-A, AP, FU). |
| [`cctv-rtsp`](crates/cctv-rtsp) | [![v](https://img.shields.io/crates/v/cctv-rtsp.svg)](https://crates.io/crates/cctv-rtsp) | [![docs](https://docs.rs/cctv-rtsp/badge.svg)](https://docs.rs/cctv-rtsp) | Pure session codecs: RTSP request/response framing, SDP parsing, and `$`-interleaved RTP stream demuxing. |
| [`cctv-onvif`](crates/cctv-onvif) | [![v](https://img.shields.io/crates/v/cctv-onvif.svg)](https://crates.io/crates/cctv-onvif) | [![docs](https://docs.rs/cctv-onvif/badge.svg)](https://docs.rs/cctv-onvif) | ONVIF Device, Media, and PTZ services over SOAP 1.2 with WS-Security `UsernameToken` authentication. |
| [`cctv-discovery`](crates/cctv-discovery) | [![v](https://img.shields.io/crates/v/cctv-discovery.svg)](https://crates.io/crates/cctv-discovery) | [![docs](https://docs.rs/cctv-discovery/badge.svg)](https://docs.rs/cctv-discovery) | UDP multicast sweeps using SSDP `M-SEARCH` and WS-Discovery `Probe`, merged into strongly typed device results. |
| [`cctv-ezviz`](crates/cctv-ezviz) | [![v](https://img.shields.io/crates/v/cctv-ezviz.svg)](https://crates.io/crates/cctv-ezviz) | [![docs](https://docs.rs/cctv-ezviz/badge.svg)](https://docs.rs/cctv-ezviz) | Ezviz Open Platform integration, wrapping `ezviz-openapi` into the standard `CameraBackend` trait. |
| [`cctv-stream-uvc`](crates/cctv-stream-uvc) | [![v](https://img.shields.io/crates/v/cctv-stream-uvc.svg)](https://crates.io/crates/cctv-stream-uvc) | [![docs](https://docs.rs/cctv-stream-uvc/badge.svg)](https://docs.rs/cctv-stream-uvc) | UVC and PTP adapters plus the OS seam: budgeted frame reading, HAL hot-plug events, PTP capture surfaces. |
| [`cctv-agent`](crates/cctv-agent) | [![v](https://img.shields.io/crates/v/cctv-agent.svg)](https://crates.io/crates/cctv-agent) | [![docs](https://docs.rs/cctv-agent/badge.svg)](https://docs.rs/cctv-agent) | Deterministic fleet reconciler: compares desired vs observed state, issues graded remediation, maintains audit trail. |
| [`cctv-detect`](crates/cctv-detect) | [![v](https://img.shields.io/crates/v/cctv-detect.svg)](https://crates.io/crates/cctv-detect) | [![docs](https://docs.rs/cctv-detect/badge.svg)](https://docs.rs/cctv-detect) | Detection device model — zones, detection reports, and monitor settings. No I/O, no algorithms. |
| [`cctv-rs`](crates/cctv-rs) | [![v](https://img.shields.io/crates/v/cctv-rs.svg)](https://crates.io/crates/cctv-rs) | [![docs](https://docs.rs/cctv-rs/badge.svg)](https://docs.rs/cctv-rs) | Public aggregate facade: camera registry, dispatch, network discovery, and fleet reconciliation. |

## Quick Start

Add `cctv-rs` to your application:

```toml
[dependencies]
cctv-rs = "0.2.0"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Initialize the facade, discover LAN cameras, and dispatch typed commands:

```rust,no_run
use cctv_rs::{CameraConfig, CommandId, Facade};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Construct the facade with the builtin ONVIF and Ezviz driver factories
    let mut facade = Facade::new().with_builtin_drivers();

    // LAN sweep — SSDP + WS-Discovery, synchronous
    let discovered = facade.discover();
    println!("Discovered {} cameras", discovered.len());

    // Register a camera: the driver key resolves through the factory registry
    let config = CameraConfig::new("camera-01", "onvif", "192.168.1.64");
    let handle = facade.attach(config)?;

    // Dispatch any typed command through the universal entry point
    let result = facade.execute(handle.id(), CommandId::GetCameraFrame).await?;
    println!("{:?}", result);

    Ok(())
}
```

## Technical Core Principles

1. **Zero Unsafe Code:** `#![forbid(unsafe_code)]` is enforced at the root of every crate in this workspace. Memory safety and slice bounds checking are guaranteed by Rust.
2. **Deterministic Errors Over Panics:** Library operations never call `unwrap()`, `expect()`, or `panic!()`. Unsupported camera capabilities yield explicit, typed `CameraError::Unsupported` variants.
3. **`no_std` / `alloc`-only Core Contract:** `cctv-core` stays dependency-lean (`serde`, `thiserror`, `async-trait`) and never pulls Tokio or network sockets into its graph.
4. **Data-Driven TLS Policy:** TLS parameters (pinned certs, minimum TLS version, ALPN preferences) are modeled as plain data, allowing runtime configuration adjustments without re-compilation.

## Verification & Testing

```bash
# Check compilation across all crates
cargo check --workspace --all-targets

# Run unit, integration, and doctests
cargo test --workspace
```

## License & Security

- **License:** MIT — See [LICENSE](LICENSE) for details.
- **Security:** Private vulnerability disclosures — See [SECURITY.md](SECURITY.md).
