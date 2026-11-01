# Release Roadmap

System development status, phase gates, locked design decisions, and future milestones.

---

## Development Phases

| Phase | Deliverable | Verification Gate | Status |
| --- | --- | --- | --- |
| 1 | `cctv-core`: `alloc`-only device model, error taxonomy, state machine, mock backend, conformance suite. | `cargo check -p cctv-core --no-default-features` clean, zero Tokio dependencies. | Completed |
| 2 | `cctv-transport` (TLS policy, budgeted frame reader), `cctv-discovery` (SSDP/WS-Discovery sweeps), `cctv-rtp` (zero-dep H.264/H.265 depacketizer), `cctv-rtsp` (RTSP framing & SDP). | Workspace unit tests passing; RTP & RTSP codecs verified against sample byte vectors. | Completed |
| 3 | `cctv-onvif`: ONVIF driver — Device, Media, and PTZ SOAP builders/parsers with WS-Security `UsernameToken`. | Conformance test suite 7/7 passing against mock SOAP server fixture. | Completed |
| 4 | `cctv-ezviz`: Ezviz Open Platform integration — token management, stream resolution, PTZ, alarms. | Driver unit tests passing against mock HTTP fixture server. | Completed |
| 5 | `cctv-rs`: Aggregate facade — driver registry, command dispatch, network discovery sweeps. | `cargo check -p cctv-rs` clean; facade integration tests passing. | Completed |
| 6 | `cctv-agent`: Fleet reconciler — desired state comparison, graded remediation actions, audit logging. | Reconciler unit tests passing. | Completed |
| 7 | Application Integration: Replace legacy camera adapters in host applications with `cctv-rs`. | End-to-end integration verification in host application. | In Progress |

### Open Items

- **Host integration (Phase 7 / plan-v0.2 Phases A–E):** `Simple-Signage` does
  not yet depend on `cctv-rs` — no `cctv_rs` reference exists in its
  `src-tauri` camera modules. The RTSP socket loop (`W3`) has landed, so the
  `camera_get_stats` swap-order prerequisite is met.
- **plan-v0.2 W4 + W5 complete (2026-11-01):** ONVIF `Discoverable` delegate
  and the agent's active remediation loop (a `CommandRelay` issues the
  commands a remediation authorizes). Milestone 0.3.0 still needs **W6**
  (vendor drivers); 0.4.0 needs W7–W9.
- **crates.io drift:** `cctv-rtp`, `cctv-rtsp`, `cctv-stream-uvc` and
  `cctv-detect` have never been published; the facade's dry run warns until
  they land. `cctv-detect` is also missing from `scripts/publish.sh`'s order.
- **`cctv-detect` wired (2026-11-01):** the facade owns the zone surface
  (`zone_save`/`record_detection`/`motion_events`, `ZoneMotionListEvents` via
  `execute`); the crate is in the publish order with its README.  What remains
  is the *application* side: feeding detections from the frame pipeline.

---

## Locked Architectural Decisions

1. **Standalone Workspace:** `cctv-rs` is an independent, pure-Rust workspace published to crates.io.
2. **Explicit Member Layout:** Workspace crates are explicitly enumerated in `Cargo.toml` to guarantee release order and build reproducibility.
3. **Protocol Decoupling:** Every protocol lives in its own driver crate (`cctv-onvif`, `cctv-ezviz`). Drivers do not leak vendor-specific types or logic into `cctv-core` or `cctv-rs`.
4. **ONVIF Trait Abstraction:** ONVIF is treated as a protocol, not a single vendor. `cctv-onvif` provides shared SOAP parsing and WS-Security helpers usable across all ONVIF-compliant hardware.
5. **Publish Scope:** `cctv-core` defines the immutable device model contract; `cctv-rs` publishes as the aggregate facade.
