# Release Roadmap

System development status, release phases, and locked architectural decisions.

---

## Phase Overview

| Phase | Milestone Scope | Verification Criteria | Status |
| --- | --- | --- | --- |
| 1 | `cctv-core`: alloc-only device model, error taxonomy, state machine, mock backend. | `cargo check -p cctv-core --no-default-features` clean, zero Tokio dependencies. | Completed |
| 2 | `cctv-transport`, `cctv-rtp`, `cctv-rtsp`: pure codecs, TLS policy, frame reading. | Workspace unit tests pass; RTP/RTSP codecs verified against RFC test vectors. | Completed |
| 3 | `cctv-driver` (`onvif` feature): SOAP builders/parsers, skewed-clock WS-Security `UsernameToken`. | Conformance tests pass against mock SOAP server fixtures. | Completed |
| 4 | `cctv-driver` (`ezviz` feature): Ezviz Open Platform cloud integration, token manager, PTZ mapping. | Unit tests pass against mock HTTP server fixture. | Completed |
| 5 | `cctv-driver` (`uvc` / `uvc-ptp` / `ddc` features): local video Ports & Adapters seam, UVC, PTP, HAL hot-plug, DDC/CI display control. | Unit tests pass across platform mocks. | Completed |
| 6 | Driver consolidation: `cctv-discovery`, `cctv-ezviz`, `cctv-stream-uvc` and `cctv-onvif` fold into `cctv-driver` behind features; `DriverAdapter` / `CameraBackend` split. | `cargo check -p cctv-driver --features ezviz,uvc,uvc-ptp,ddc` clean; workspace tests pass. | Completed |
| 7 | `cctv-agent`: Fleet reconciler with graded remediation, `StreamMux`, `SnapshotProxy`. | Reconciler and stream multiplexer test suites pass. | Completed |
| 8 | `cctv-detect`, `cctv-pair`, `cctv-sync` & `cctv-rs`: Detection device model, pairing/trust, CRDT convergence, facade registry, universal dispatch. | Full workspace tests and facade integration suite pass. | Completed |
| 9 | Host Application Integration: Production deployment in edge video management clients. | Integration verification in edge host application. | In Progress |

---

## Current Status (v0.3.0 Released)

### 0.3.0 — Driver Expansion & Streaming Hardening (2026-11-02) **Released**

Twelve workspace crates published to crates.io (`cctv-core`, `cctv-transport`,
`cctv-rtp`, `cctv-rtsp`, `cctv-driver`, `cctv-onvif`, `cctv-agent`,
`cctv-detect`, `cctv-rag`, `cctv-pair`, `cctv-sync`, `cctv-rs`) plus
`ezviz-openapi` (own repository, consumed by `cctv-driver`'s `ezviz` feature).
`cctv-onvif` is retained as a deprecated re-export shim for one release.

Since the 0.3.0 release the vendor adapters have consolidated: `cctv-discovery`,
`cctv-ezviz`, `cctv-stream-uvc` and `cctv-onvif` are now features of
`cctv-driver` (`discovery` and `onvif` — both default, `ezviz`, `uvc`,
`uvc-ptp`, `ddc`), reducing the published surface from fifteen crates to twelve.

W6 delivered:
- **RTSP session reconnection hardening** — bounded exponential backoff
  (`RECONNECT_MAX_DELAY` 10 s, `RECONNECT_MAX_RETRIES` 8), `StreamEnd::Stalled`
  for a silent session, `StreamEnd::Failed` for a hard failure;
  `cctv_rtsp::streaming::run` keeps the media channel alive while the loop
  re-arms.
- **Live stream health watchdog** — `cctv_core::health` state machine
  (`HealthWatchdog`, healthy/stalled/dead, alloc-only) wired into the driver
  stream path through `READ_STALL` and the reconnect policy.

Quirks fixed along the way: the request-response digest retry, the interleaved
media channel survival, and the stale-frame drop path in `FrameReader`.

### 0.4.0 — Fleet Operations & Edge Storage (planned)

- Edge storage caching primitives.
- Multi-stream composite recording profiles.
- Advanced fleet reconciliation policies.

---

## Locked Decisions

1. **Pure-Rust Workspace**: No C-FFI, no vendor binary blobs. All crates are
   licensed under Apache-2.0.
2. **Explicit Manifest**: Workspace members are explicitly declared in
   `Cargo.toml` in dependency publish order.
3. **Decoupled Drivers**: Protocols reside in standalone crates. Drivers do not
   pollute `cctv-core` with vendor concepts.
4. **Alloc-Only Contract**: `cctv-core` has zero runtime or socket
   dependencies.
5. **No Native TLS**: Network transport relies strictly on `rustls` for
   deterministic cross-platform behavior.
6. **Health as a first-class contract**: a live stream is a health state, not a
   boolean (cctv_core::health, cctv_rtsp::streaming reconnect policy).
