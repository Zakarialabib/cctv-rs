# cctv-agent

[![Crates.io](https://img.shields.io/crates/v/cctv-agent.svg)](https://crates.io/crates/cctv-agent)
[![docs.rs](https://docs.rs/cctv-agent/badge.svg)](https://docs.rs/cctv-agent)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Deterministic fleet reconciliation engine for `cctv-rs`. Compares desired fleet configuration against observed camera state and applies graded remediation actions.

## Graded Remediation Model

Instead of binary on/off states, `cctv-agent` grades failures to avoid unnecessary service disruptions:

| Severity Grade | Remediation Action | Trigger |
| --- | --- | --- |
| `None` | `Hold` | Observed state matches desired state. |
| `Observe` | `Hold` | Capability disabled or unverified. |
| `Notify` | `Hold` / `Advance` | Transient state transition or unknown error. |
| `Retry` | `MarkOffline` | Transient socket / network dropped packet. |
| `Reconfigure` | `TakeOutOfService` | Authentication error or operator disable. |
| `Disable` | `TakeOutOfService` | Camera missing from hardware registry. |

## Audit Logging

Every state transition and remediation decision records an append-only `AuditEntry` containing timestamps, camera handle, remediation grade, and actionable cause.

## Active Remediation (`CommandRelay`)

`Remediation::commands` names the `CommandId`s a grade authorizes — `MarkOffline` → `TestCameraConnection`, `TakeOutOfService` → `CameraSetEnabled`, `Advance(Streaming|Recording)` → `StartCameraStream`, `Advance(Retrying)` → `TestCameraConnection`. `Hold` and non-actionable grades issue nothing, and an authentication failure never commands: only an operator can supply credentials.

`Reconciler::with_command_relay` makes issuance active — commands flow through the sync relay as they are graded, each audited as `issued <CommandId>`. Without a relay the reconciler stays report-only. `CommandRelay` and reconciler issuance stay I/O-free — connections belong to the crate's runner and stream modules.

## Streaming & Session Surfaces

- **`StreamMux`:** GOP-cached NALU multiplexer for RTSP sub-streams — pulls the sub-stream once (`cctv-rtp` + `cctv-transport`), caches the latest parameter sets and keyframe, and broadcasts NAL units so a client joining `GET /agent/cameras/{id}/live_ws` renders frame 1 with zero GOP wait.
- **`SnapshotProxy`:** native snapshot retrieval behind a 500 ms in-memory cache with RTSP Digest authentication — concurrent or rapid requests neither consume camera RTSP sessions nor burn host CPU.
- **`SessionManager` / `CameraCapabilities`:** session-budget bookkeeping re-exported alongside the provisioning types (`DecryptedCameraAuth`, `StreamProfileSet`, …).

## Usage Example

```rust,no_run
use cctv_agent::Reconciler;

let mut reconciler = Reconciler::new();
// Configure desired camera state and run reconciliation pass
```

## Technical Guarantees

- `#![forbid(unsafe_code)]`
- Deterministic, side-effect-free decision evaluation.
