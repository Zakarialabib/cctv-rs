# cctv-agent

[![Crates.io](https://img.shields.io/crates/v/cctv-agent.svg)](https://crates.io/crates/cctv-agent)
[![docs.rs](https://docs.rs/cctv-agent/badge.svg)](https://docs.rs/cctv-agent)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Deterministic fleet reconciliation engine, sub-stream multiplexer, and snapshot cache for `cctv-rs`.

## Primitives

- **Graded Remediation (`reconcile`)**: Evaluates desired configuration against observed camera state and assigns graded actions:
  - `Hold`: Observed matches desired, or non-actionable state.
  - `Advance`: Progressive state promotion (e.g. initiating stream playback).
  - `MarkOffline`: Transient network or packet loss remediation.
  - `TakeOutOfService`: Persistent authentication failure or missing device removal.
- **Active Remediation (`CommandRelay`)**: When configured with a `CommandRelay`, automatically executes authorized `CommandId` instructions during sweeps.
- **Append-Only Audit**: Every remediation decision produces an immutable `AuditEntry`.
- **GOP-Cached Multiplexer (`StreamMux`)**: Caches critical NAL units (`[SPS, PPS, (VPS), IDR]`) to allow new clients to start rendering immediately without waiting for the next keyframe.
- **Throttled Snapshot Proxy (`SnapshotProxy`)**: Caches camera snapshots with RTSP Digest auth behind a 500 ms window to prevent camera resource exhaustion.

## Invariants

- `#![forbid(unsafe_code)]`
- Reconciliation logic is purely computational and free of ambient I/O.

## Usage

```rust,no_run
use cctv_agent::{Grade, Reconciler};
use cctv_core::{CameraDesired, CameraHandle, CameraStateCurrent};

let reconciler = Reconciler::default();
let handle = CameraHandle::new("cam-01");
let desired = CameraDesired::new(handle.clone(), true);
let current = CameraStateCurrent::new(handle);

let (audit, remediation) = reconciler.reconcile(&desired, &current);
assert_eq!(remediation.grade(), Grade::None);
```
