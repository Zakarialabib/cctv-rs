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
| [`cctv-agent`](crates/cctv-agent) | [![v](https://img.shields.io/crates/v/cctv-agent.svg)](https://crates.io/crates/cctv-agent) | [![docs](https://docs.rs/cctv-agent/badge.svg)](https://docs.rs/cctv-agent) | Deterministic fleet reconciler: compares desired vs observed state, issues graded remediation, maintains audit trail. |
| [`cctv-protocol-ptp`](crates/cctv-protocol-ptp) | [![v](https://img.shields.io/crates/v/cctv-protocol-ptp.svg)](https://crates.io/crates/cctv-protocol-ptp) | [![docs](https://docs.rs/cctv-protocol-ptp/badge.svg)](https://docs.rs/cctv-protocol-ptp) | Picture Transfer Protocol (PTP / ISO 15740 / PTP-IP) PDU codecs, session management client, and mock responder. |
| [`cctv-storage-msc`](crates/cctv-storage-msc) | [![v](https://img.shields.io/crates/v/cctv-storage-msc.svg)](https://crates.io/crates/cctv-storage-msc) | [![docs](https://docs.rs/cctv-storage-msc/badge.svg)](https://docs.rs/cctv-storage-msc) | Read-only Mass Storage Class (MSC) FAT32 filesystem parser: BPB, FAT cluster chain traversal, 8.3 & LFN directory parsing. |
| [`cctv-adapter-actioncam`](crates/cctv-adapter-actioncam) | [![v](https://img.shields.io/crates/v/cctv-adapter-actioncam.svg)](https://crates.io/crates/cctv-adapter-actioncam) | [![docs](https://docs.rs/cctv-adapter-actioncam/badge.svg)](https://docs.rs/cctv-adapter-actioncam) | Action camera adapter: passive mDNS TXT identification for Insta360 X4 and read-only status endpoint inspection. |
| [`cctv-rs`](crates/cctv-rs) | [![v](https://img.shields.io/crates/v/cctv-rs.svg)](https://crates.io/crates/cctv-rs) | [![docs](https://docs.rs/cctv-rs/badge.svg)](https://docs.rs/cctv-rs) | Public aggregate facade: camera registry, dispatch, network discovery, and fleet reconciliation. |

## Quick Start

Add `cctv-rs` to your application:

```toml
[dependencies]
cctv-rs = "0.1.0"
```

Initialize the facade, discover LAN cameras, and dispatch typed commands:

```rust,no_run
use cctv_rs::Facade;
use cctv_core::{CameraConfig, CameraHandle, CommandId};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Construct facade with builtin ONVIF and Ezviz drivers
    let facade = Facade::with_builtin_drivers();

    // Perform an asynchronous LAN sweep (SSDP + WS-Discovery)
    let discovered = facade.discover().await?;
    println!("Discovered {} cameras", discovered.len());

    // Register a camera into the fleet
    let handle = CameraHandle::from("camera-01");
    let config = CameraConfig::new("camera-01", "192.168.1.64", "onvif");
    facade.register(handle.clone(), config).await?;

    // Dispatch typed commands
    let result = facade.execute(handle.as_str(), CommandId::GetCameraFrame).await?;

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
