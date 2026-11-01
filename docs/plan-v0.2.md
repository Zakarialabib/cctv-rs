# Execution Plan: v0.2 → v0.4

Work packages, release targets, and verification criteria for `cctv-rs`.

---

## Release Milestones

| Milestone | Target Scope | Key Deliverables |
| --- | --- | --- |
| **0.2.0** | Core & Streaming | Core `alloc`-only refactor (W1), `cctv-transport` socket layer (W2), RTSP `CameraBackend` driver (W3). **Released 2026-11-01** — the release also carries W4, W5, the facade zone surface, PR #36 auth, and PR #38 StreamMux/SnapshotProxy. |
| **0.3.0** | Discovery & Agent | ONVIF `Discoverable` implementation (W4), active agent remediation loop (W5), vendor drivers (W6). *(W4/W5 shipped inside 0.2.0; this milestone now hinges on W6.)* |
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

### W4 — `cctv-onvif`: WS-Discovery Delegate ✅ (complete 2026-11-01)
- **Goal:** Implement `Discoverable` trait delegating UDP multicast sweeps to `cctv-discovery`.
- **Verification Gate:**
  ```bash
  cargo test -p cctv-onvif
  ```
- **Outcome:** `cctv-onvif::discovery` — `Discoverable` (object-safe, `Send + Sync`)
  and `OnvifDiscovery`, filtering `wsd::parse_probe_matches` results by ONVIF
  evidence and an HTTP(S) `XAddr`, converting via `cctv_discovery::probe`.
  `cctv-discovery`'s `message_id` and `camera_from_probe_match` are published
  as the delegation seams; publish order swapped (`discovery` before `onvif`).

### W5 — `cctv-agent`: Active Remediation ✅ (complete 2026-11-01)
- **Goal:** Enable the reconciler to issue remediation commands directly when state drift is detected.
- **Verification Gate:**
  ```bash
  cargo test -p cctv-agent
  ```
- **Outcome:** `Remediation::commands` derives the authorized `CommandId`s from
  the action (hold issues nothing; offline → connection test; out-of-service →
  set-enabled; streaming/recording drift → start-stream; retrying drift →
  connection test).  `Reconciler::with_command_relay(Box<dyn CommandRelay>)`
  issues them through the sync relay as they are graded, auditing each as
  `issued <CommandId>`; no relay = report-only, unchanged.  Exported from
  `cctv-rs`.  **Milestone 0.3.0: W4 + W5 complete; W6 (vendor drivers) open.**

---

## Host Application Integration Phases

1. **Phase A — ✅ done (2026-11-01):** `Simple-Signage` depends on `cctv-rs`
   (git-pinned `a4a827f`, post-W3/W4/W5 — crates.io 0.1.0 predates the RTSP
   driver); `Facade::new().with_builtin_drivers()` is constructed and managed
   as Tauri state behind `Mutex`, desktop-gated with the camera stack.
   Landed in SignageMaster PRs #722 + #723. Note: its adoption matrix's
   "reject facade" row was overridden by owner decision — the matrix records
   the override.
2. **Phase B:** Migrate LAN network discovery and stream URI resolution to `cctv-rs`.
3. **Phase C:** Connect application video consumers to `cctv-rs` stream readers.
4. **Phase D:** Migrate PTZ, preset, and snapshot commands to `cctv-rs` facade entry points.
5. **Phase E:** Remove legacy in-house camera code and complete integration.
