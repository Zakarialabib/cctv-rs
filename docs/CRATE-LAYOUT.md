# Crate Layout

This is the map of `cctv-rs` as it ships today. Read it with the 
[`ARCHITECTURE-SPEC.md`](ARCHITECTURE-SPEC.md) — the two documents are one design.

Each crate has a single reason to exist, and the only crate the app touches is
`cctv` (the facade).

## Crate count and roles

| Crate | Dependency graph | Role | No tokio | No protocol |
|---|---|---|---|---|
| `cctv-core` | none | Device model: types, state, commands, errors, mocks, quirks | **yes** | **yes** |
| `cctv-ezviz` | `cctv-core`, standalone `ezviz-openapi-rs` | Ezviz Open Platform driver (token, device list, stream, PTZ, alarms, capture) | **no** | **no** |
| `cctv-discovery` | `cctv-transport` | SSDP / multicast / USB camera discovery | no | **yes** |
| `cctv-transport` | none | TLS policy + budgeted frame reader (policy is data) | no | no |
| `cctv-agent` | `cctv-core` | Fleet reconciler: desired state → graded remediation, audit trail | no | no |
| `cctv` | all five above | **Facade** — the only crate the app talks to | no | no |

## Interfaces

```
┌─ SignageMaster ───────────────────────────────────────────────┐
│ src/core/cctv.ts                     (pure facade)            │
│   → cctv (crates/cctv)                                   │
└──────────────┬──────────────────────────────────────────────┘
               │  cctv-core device model + event bus
        ┌──────┴──────┐
        │             ▼
        │   cctv-core / cctv-agent / cctv-transport / cctv-discovery
        │             ▲
        └──────┬──────┘
               │  driver crates: protocol only
        ┌──────┴──────┐
        ▼             ▼
┌───────────────┐ ┌───────────────┐ ┌───────────────┐
│ cctv-ezviz    │ │ planned:      │ │ planned:      │
│ (Ezviz Open  │ │ hikvision-rs  │ │ dahua-rs      │
│  API)         │ │ (one crate)   │ │ (one crate)   │
└───────────────┘ └───────────────┘ └───────────────┘
```

## Principles

- **Vendor never reaches the app.** `cctv.ts` sees only `CameraHandle` +
  `CameraError` from `cctv-core`. Vendors live in their own crates.
- **`cctv-core` is embeddable.** It has no tokio, no protocol, no HTTP. A CMS
  can `use cctv_core` without pulling in a single driver.
- **Drivers are thin.** `cctv-ezviz` is a path dep on the standalone
  `ezviz-openapi-rs`; it must not bring new device-model logic into the app.
- **Policy is data.** TLS, quirks and the driver list live in data files /
  loaders so a deployment can change behaviour without recompiling.
