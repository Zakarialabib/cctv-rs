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

## Usage Example

```rust,no_run
use cctv_agent::Reconciler;

let mut reconciler = Reconciler::new();
// Configure desired camera state and run reconciliation pass
```

## Technical Guarantees

- `#![forbid(unsafe_code)]`
- Deterministic, side-effect-free decision evaluation.
