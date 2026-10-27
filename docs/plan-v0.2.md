# Execution Plan: v0.2 → v0.4

Work packages, release targets, and verification criteria for `cctv-rs`.

---

## Release Milestones

| Milestone | Target Scope | Key Deliverables |
| --- | --- | --- |
| **0.2.0** | Core & Streaming | Core `alloc`-only refactor (W1), `cctv-transport` socket layer (W2), RTSP `CameraBackend` driver (W3). |
| **0.3.0** | Discovery & Agent | ONVIF `Discoverable` implementation (W4), active agent remediation loop (W5), vendor drivers (W6). |
| **0.4.0** | Vendor Expansion | Additional vendor protocol drivers (W7, W8), facade polish (W9). |

---

## Work Packages

### W1 — `cctv-core`: `alloc`-Only Contract
- **Goal:** Enable `#![no_std]` + `alloc` support for `cctv-core`. Adopt `thiserror` and `async-trait`.
- **Verification Gate:**
  ```bash
  cargo check -p cctv-core --no-default-features
  cargo test --workspace
  ```

### W2 — `cctv-transport`: Socket & TLS Layer
- **Goal:** Implement the `net` feature providing TCP/TLS connection helpers and `FrameReader` stream integration.
- **Verification Gate:**
  ```bash
  cargo test -p cctv-transport --features net
  ```

### W3 — `cctv-rtsp`: RTSP Driver Session
- **Goal:** Complete RTSP stream handshake (DESCRIBE → SETUP → PLAY) over TCP sockets with Digest auth.
- **Verification Gate:**
  ```bash
  cargo test -p cctv-rtsp
  ```

### W4 — `cctv-onvif`: WS-Discovery Delegate
- **Goal:** Implement `Discoverable` trait delegating UDP multicast sweeps to `cctv-discovery`.
- **Verification Gate:**
  ```bash
  cargo test -p cctv-onvif
  ```

### W5 — `cctv-agent`: Active Remediation
- **Goal:** Enable the reconciler to issue remediation commands directly when state drift is detected.
- **Verification Gate:**
  ```bash
  cargo test -p cctv-agent
  ```

---

## Host Application Integration Phases

1. **Phase A:** Add `cctv-rs` as a dependency; construct `Facade` instance.
2. **Phase B:** Migrate LAN network discovery and stream URI resolution to `cctv-rs`.
3. **Phase C:** Connect application video consumers to `cctv-rs` stream readers.
4. **Phase D:** Migrate PTZ, preset, and snapshot commands to `cctv-rs` facade entry points.
5. **Phase E:** Remove legacy in-house camera code and complete integration.
