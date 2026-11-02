# Release Roadmap

System development status, release phases, and locked architectural decisions.

---

## Phase Overview

| Phase | Milestone Scope | Verification Criteria | Status |
| --- | --- | --- | --- |
| 1 | `cctv-core`: alloc-only device model, error taxonomy, state machine, mock backend. | `cargo check -p cctv-core --no-default-features` clean, zero Tokio dependencies. | Completed |
| 2 | `cctv-transport`, `cctv-discovery`, `cctv-rtp`, `cctv-rtsp`: pure codecs, TLS policy, frame reading. | Workspace unit tests pass; RTP/RTSP codecs verified against RFC test vectors. | Completed |
| 3 | `cctv-onvif`: SOAP builders/parsers, skewed-clock WS-Security `UsernameToken`. | Conformance tests pass against mock SOAP server fixtures. | Completed |
| 4 | `cctv-ezviz`: Ezviz Open Platform cloud integration, token manager, PTZ mapping. | Unit tests pass against mock HTTP server fixture. | Completed |
| 5 | `cctv-stream-uvc`: Local video Ports & Adapters seam, UVC, PTP, HAL hot-plug. | Unit tests pass across platform mocks. | Completed |
| 6 | `cctv-agent`: Fleet reconciler with graded remediation, `StreamMux`, `SnapshotProxy`. | Reconciler and stream multiplexer test suites pass. | Completed |
| 7 | `cctv-detect` & `cctv-rs`: Detection device model, facade registry, universal dispatch. | Full workspace tests and facade integration suite pass. | Completed |
| 8 | Host Application Integration: Production deployment in edge video management clients. | Integration verification in edge host application. | In Progress |

---

## Current Status (v0.2.0 Shipped)

- **0.2.0 Released**: All 11 crates published to crates.io (`cctv-core`, `cctv-transport`, `cctv-rtp`, `cctv-rtsp`, `cctv-discovery`, `cctv-onvif`, `cctv-ezviz`, `cctv-stream-uvc`, `cctv-agent`, `cctv-detect`, `cctv-rs`).
- **Active Remediation**: `CommandRelay` issues authorized `CommandId`s during reconciliation sweeps.
- **`StreamMux` & `SnapshotProxy`**: GOP-cached sub-stream multiplexer and throttled snapshot proxy operational in `cctv-agent`.
- **Zone & Detection Surface**: `Facade` provides complete zone configuration and detection event evaluation via `cctv-detect`.

---

## Locked Decisions

1. **Pure-Rust Workspace**: No C-FFI, no vendor binary blobs. All 11 crates published under MIT license.
2. **Explicit Manifest**: Workspace members are explicitly declared in `Cargo.toml` in dependency publish order.
3. **Decoupled Drivers**: Protocols reside in standalone crates. Drivers do not pollute `cctv-core` with vendor concepts.
4. **Alloc-Only Contract**: `cctv-core` has zero runtime or socket dependencies.
5. **No Native TLS**: Network transport relies strictly on `rustls` for deterministic cross-platform behavior.
