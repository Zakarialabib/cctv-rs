# cctv-rs

[![Crates.io](https://img.shields.io/crates/v/cctv-rs.svg)](https://crates.io/crates/cctv-rs)
[![docs.rs](https://img.shields.io/docsrs/cctv-rs/badge.svg)](https://docs.rs/cctv-rs)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)
[![Build Status](https://img.shields.io/github/actions/workflow/status/Zakarialabib/cctv-rs/ci.yml?branch=main)](https://github.com/Zakarialabib/cctv-rs/actions)

Modular, zero-panic, pure-Rust CCTV backend engine. Discover, stream, control, and
reconcile IP camera fleets across ONVIF, Ezviz, RTSP, and local video sources
behind a single typed facade.

No C bindings, no vendor SDK bloat, no UI, no database dependencies, and
`#![forbid(unsafe_code)]` everywhere.

---

## Workspace Crates

Eleven published crates in publish order, plus the unpublished `xtask` task runner
(twelve workspace members).

| Crate | Crates.io | Description |
| --- | --- | --- |
| [`cctv-core`](crates/cctv-core) | [![v](https://img.shields.io/crates/v/cctv-core.svg)](https://crates.io/crates/cctv-core) | `alloc`-only device model: typed commands, error taxonomy, state machine & mock suite. Zero Tokio, zero async runtime. |
| [`cctv-transport`](crates/cctv-transport) | [![v](https://img.shields.io/crates/v/cctv-transport.svg)](https://crates.io/crates/cctv-transport) | Budgeted `FrameReader`, zero-alloc XML scanner, pure TCP/TLS socket abstractions (`rustls` only). |
| [`cctv-rtp`](crates/cctv-rtp) | [![v](https://img.shields.io/crates/v/cctv-rtp.svg)](https://crates.io/crates/cctv-rtp) | Zero-dependency RTP header parsing & H.264/H.265 access-unit depacketization (STAP-A, FU-A, AP, FU). |
| [`cctv-rtsp`](crates/cctv-rtsp) | [![v](https://img.shields.io/crates/v/cctv-rtsp.svg)](https://crates.io/crates/cctv-rtsp) | Pure RTSP framing, SDP track extraction, `$`-interleaved demuxing, and RFC 2617/7616 Digest auth. |
| [`cctv-driver`](crates/cctv-driver) | [![v](https://img.shields.io/crates/v/cctv-driver.svg)](https://crates.io/crates/cctv-driver) | Vendor-neutral adapter seam: `DriverAdapter` capability surface + `DriverRegistry`, plus the bundled ONVIF (`onvif`), Ezviz (`ezviz`), UVC/PTP (`uvc`, `uvc-ptp`, `ddc`) and discovery sweeps behind features. |
| [`cctv-agent`](crates/cctv-agent) | [![v](https://img.shields.io/crates/v/cctv-agent.svg)](https://crates.io/crates/cctv-agent) | Deterministic fleet reconciler with graded remediation, audit trail, GOP-cached `StreamMux`, `SnapshotProxy`. |
| [`cctv-detect`](crates/cctv-detect) | [![v](https://img.shields.io/crates/v/cctv-detect.svg)](https://crates.io/crates/cctv-detect) | Pure detection device model: zones, detection reports, monitor settings. No I/O, no heavy ML deps. |
| [`cctv-rag`](crates/cctv-rag) | [![v](https://img.shields.io/crates/v/cctv-rag.svg)](https://crates.io/crates/cctv-rag) | Deterministic RAG: temporal incident ledger, spatial topology graph, BM25 retriever, and diagnostic playbooks. No async runtime, no external database. |
| [`cctv-pair`](crates/cctv-pair) | [![v](https://img.shields.io/crates/v/cctv-pair.svg)](https://crates.io/crates/cctv-pair) | Device pairing ceremony and trust store — certificate-secured pairing protocol, `TrustStore`, pairing URI parsing. |
| [`cctv-sync`](crates/cctv-sync) | [![v](https://img.shields.io/crates/v/cctv-sync.svg)](https://crates.io/crates/cctv-sync) | Synchronous device registry convergence: graded re-sync loops with tie-breaking. |
| [`cctv-rs`](crates/cctv-rs) | [![v](https://img.shields.io/crates/v/cctv-rs.svg)](https://crates.io/crates/cctv-rs) | Unified aggregate facade: driver registry, universal dispatch, discovery sweeps, zone management. |

---

## Quick Start

Add `cctv-rs` to `Cargo.toml`:

```toml
[dependencies]
cctv-rs = "0.4.0"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Spin up the facade, discover LAN hardware, and dispatch commands:

```rust,no_run
use cctv_rs::{CameraConfig, CommandId, Facade};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Facade initialized with builtin ONVIF + Ezviz driver factories
    let mut facade = Facade::new().with_builtin_drivers();

    // Synchronous LAN sweep (SSDP + WS-Discovery)
    let discovered = facade.discover();
    println!("Discovered {} cameras", discovered.len());

    // Register camera configuration
    let config = CameraConfig::new("camera-01", "onvif", "192.168.1.64");
    let handle = facade.attach(config)?;

    // Dispatch typed command through the universal entry point
    let frame = facade.execute(handle.id(), CommandId::GetCameraFrame).await?;
    println!("Frame fetched: {:?}", frame);

    Ok(())
}
```

---

## Engineering Invariants

- **`#![forbid(unsafe_code)]`** — enforced on every crate root. Zero pointer
  arithmetic, zero undefined-behaviour risk.
- **Zero panics** — library code never calls `unwrap()`, `expect()`, `panic!()`,
  or `todo!()`. Missing features return explicit `CameraError::Unsupported`.
- **`alloc`-only core** — `cctv-core` builds with `#![no_std]` + `alloc`. No
  Tokio, no sockets, no background threads in the contract.
- **Data-driven TLS** — TLS configuration (`Policy`) is pure data: pinned certs,
  ciphers, and ALPN rules without recompilation.
- **Zeroized secrets** — in-memory camera passwords are wiped on drop via
  `zeroize::Zeroizing<String>` with redacted `Debug`.

---

## Verification

```bash
cargo fmt --all --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

---

## Documentation

The full documentation set is numbered — start at the index:

- [00 — Documentation Index](docs/00-index.md)
- [01 — System Architecture](docs/01-architecture.md)
- [02 — Engineering Standard](docs/02-rust-package-standard.md)
- [03 — Command Catalogue](docs/03-commands.md)
- [04 — Camera Taxonomy](docs/04-camera-types.md)
- [05 — QUIC Tuning](docs/05-quic-tuning.md)
- [06 — Roadmap](docs/06-roadmap.md)
- [07 — Execution Plan](docs/07-plan.md)

---

## License & Security

- **License:** Apache-2.0 ([LICENSE](LICENSE))
- **Security:** vulnerability disclosure process ([SECURITY.md](SECURITY.md))
