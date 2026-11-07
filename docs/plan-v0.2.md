# Execution Plan: v0.2 → v0.4

Work packages, release milestones, and verification targets for `cctv-rs`.

---

## Release Milestones

| Milestone | Target Scope | Key Deliverables |
| --- | --- | --- |
| **0.2.0** | Core, Transport, Codecs & Agent | Alloc-only core contract, socket layer, pure RTSP/RTP codecs, ONVIF discovery delegate, active agent remediation loop, StreamMux, SnapshotProxy, and facade detection surface. **Released**. |
| **0.3.0** | Driver Expansion & Streaming Hardening | Expanded vendor protocol drivers, RTSP session reconnection hardening, live stream health watchdog enhancements. |
| **0.4.0** | Fleet Operations & Edge Storage | Edge storage caching primitives, multi-stream composite recording profiles, advanced fleet reconciliation policies. |

---

## Work Packages

### W1 — `cctv-core`: `alloc`-Only Contract ✅
- Enabled `#![no_std]` + `alloc` support for `cctv-core`. Adopted `thiserror` and `async-trait`.
- Verified via: `cargo check -p cctv-core --no-default-features`.

### W2 — `cctv-transport`: Socket & TLS Layer ✅
- Implemented `net` feature for pure `tokio` + `rustls` sockets and `FrameReader` stream parsing.
- Verified via: `cargo test -p cctv-transport --features net`.

### W3 — `cctv-rtsp`: RTSP Driver Session ✅
- Completed RTSP handshake (DESCRIBE → SETUP → PLAY) with RFC 2617/7616 Digest auth state machine.
- Verified via: `cargo test -p cctv-rtsp`.

### W4 — `cctv-onvif`: WS-Discovery Delegate ✅
- Implemented `Discoverable` trait delegating sweeps to `cctv-discovery`.
- Verified via: `cargo test -p cctv-onvif`.

### W5 — `cctv-agent`: Active Remediation Loop ✅
- Enabled reconciler to issue authorized remediation commands via `CommandRelay`.
- Shipped `StreamMux` GOP-cached NALU multiplexer and `SnapshotProxy`.
- Verified via: `cargo test -p cctv-agent`.

### W6 — Extended Vendor Drivers & Protocols (0.3.0) ✅
- RTSP session reconnection hardening: bounded exponential backoff
  (`RECONNECT_MAX_DELAY` 10 s, `RECONNECT_MAX_RETRIES` 8), `StreamEnd::Stalled`
  classification, health-state reporting on reconnect failure.
- Live stream health watchdog: `cctv_core::health` (HealthWatchdog, alloc-only:
  healthy/stalled/dead), wired into the driver stream path through `READ_STALL`.
- Verified via: `RECONNECT_MAX_DELAY` + `RECONNECT_MAX_RETRIES` unit tests, the
  cctv_core health module tests, the rtsp_fixture end-to-end suite.

---

## Host Application Integration Phases

1. **Phase A (Complete)**: Host edge client adopts `cctv-rs = "0.2.0"`; constructs and stores `Facade::new().with_builtin_drivers()` in application state.
2. **Phase B (In Progress)**: Migrate network discovery sweeps and RTSP stream URI resolution to `cctv-rs`.
3. **Phase C**: Connect application video consumers to `cctv-rs` stream readers and `StreamMux` feeds.
4. **Phase D**: Migrate PTZ controls, presets, snapshots, and detection zones to facade commands.
5. **Phase E**: Deprecate and remove legacy camera driver adapters.
