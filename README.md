# cctv-rs

A pure-Rust CCTV backend for SignageMaster: discover, stream, control (PTZ), analyse
(zones / motion / AI) and reconcile camera fleets — no Tauri, no UI, no SQLite.

It is the camera half of the architecture refactor: `src-tauri` hands the app its
typed facade, and the driver for every camera lives in `cctv-ezviz` (the reverse-
engineered Ezviz Open Platform client, `ezviz-openapi-rs`).

## Workspace

```
|cctv-core          device model: traits, state, errors, mock, conformance (no tokio)
|cctv               facade: discover, frame, ptz, start/stop, stats, config, presets
|cctv-discovery     SSDP / multicast / USB camera discovery
|cctv-transport     TLS policy, pinning, budgeted frame reader
|cctv-ezviz         Ezviz Open-Platform driver (from ezviz-openapi-rs)
|cctv-agent         fleet reconciler: offline→armed→streaming, audit trail
```

## Quick start

```
git clone <cctv-rs>
cd cctv-rs
cargo check --workspace
cargo test --workspace
```

## What it is NOT

- Not a Tauri plugin. Nothing here owns a window or a SQLite DB.
- No vendor shipped inside the workspace: `ezviz-openapi-rs` is consumed as a
  dependency. PTZ protocols (ONVIF/RTSP) reach the app through the driver crates.
- Not a media server: `cctv-transport` only reads and buffers frames.

## Documentation

See [`docs/ARCHITECTURE-SPEC.md`](docs/ARCHITECTURE-SPEC.md) — the full architecture,
protocol matrix, command taxonomy and implementation phases.
