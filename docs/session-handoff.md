# Session Handoff — cctv-rs evolution (2026-11-07)

## Version history
- **v0.1.0** (Oct 2026): 68 commits, release pending.
- **v0.2.0** (2026-11-01): 11 workspace crates on crates.io; tag `v0.2.0`; GitHub
  release live.
- **v0.3.0** (2026-11-07): fifteen crates (14 workspace + `ezviz-openapi` from its
  own repo); tag `v0.3.0`; GitHub release live; CI 10/10 green.

## W6 — vendor-neutral driver seam (cctv-driver)
- `crates/cctv-driver`: `DriverAdapter` capability surface (probe/stream/ptz/
  presets + save|goto|delete_preset), `DriverRegistry` + `InMemoryRegistry`,
  typed `CameraError` taxonomy, `#[cfg(feature = "driver-mock")]` mock fixture.
- HikvisionDahua knowledge moved out of Simple-Signage into the general
  `FactoryDriver` trait + `DriverRegistry` + `DriverConfig` pattern.
- No HikvisionDahua naming inside cctv-rs crates/docs.

## Live-stream hardening (two sub-deliverables of the 0.3.0 scope)
1. **RTSP session reconnection hardening** (`cctv-rtsp`):
   - `RECONNECT_MAX_DELAY` (10 s) + `RECONNECT_MAX_RETRIES` (8);
   - `cctv_rtsp::streaming::run` keeps the media channel alive while re-arming;
   - `StreamEnd::Stalled` for a silent session, `StreamEnd::Failed` for a hard
     failure, `Cancelled` for a stop.
2. **Live stream health watchdog enhancements** (`cctv_core` + `cctv-rtsp`):
   - `cctv_core::health`: `Health` (Healthy/Stalled/Dead), `HealthChanged`,
     `HealthWatchdog` — alloc-only, tick-based state machine;
   - `READ_STALL` (15 s) drives the stall budget; a stalled stream stays stalled
     with its original `since_ms` until a frame renews it;
   - `cctv_rtsp` loop reports `StreamEnd::Stalled` / `::Failed` / `::Retrying` /
     `::Cancelled` and re-arms the session.

## Verification (0.3.0 release tree)
- `cargo fmt --all --check` → 0
- `cargo clippy --workspace --all-targets -- -D warnings` → 0
- `cargo test --workspace` → 28 suites, 0 failed
- `cargo doc --workspace --no-deps` → clean
- crates.io read-back: 15/15 crates HTTP 200
- Consumer smoke test (`cargo add cctv-rs@0.3.0`) → builds and runs

## Post-0.3.0 — driver consolidation (unreleased)
- `cctv-discovery`, `cctv-ezviz` and `cctv-stream-uvc` are folded into
  `cctv-driver` behind features: `discovery` (default), `ezviz`, `uvc`,
  `uvc-ptp`, `ddc`, plus `driver-mock` and `mobile`. The three standalone crates
  and their directories are gone.
- The seam is split: `DriverAdapter` is the factory surface (`driver_key`,
  `capabilities`, `probe`, `attach`); `CameraBackend` is the live handle.
- `cctv-driver` sits **before** `cctv-onvif` in `Cargo.toml` member order,
  `scripts/publish.sh` `ORDER` and CI's per-crate job — `cctv-onvif` depends on
  it for discovery.
- Published surface is now **twelve** crates (was fifteen). `ezviz-openapi`
  remains an external crates.io dependency of the `ezviz` feature.
- The docs (`README.md`, `docs/architecture.md`, `docs/roadmap.md`,
  `docs/target-architecture.md`, `CONTRIBUTING.md`, `SECURITY.md`,
  `docs/plan-v0.2.md`) were reconciled to this topology.

## What is still open (not part of the shipped 0.3.0)
- 0.4.0 milestone (not started): edge storage caching primitives,
  multi-stream composite recording profiles, advanced fleet reconciliation
  policies.
- 0.4.0 roadmap is recorded in `docs/plan-v0.2.md` and `docs/roadmap.md`.

## Files changed in this evolution record
- `CHANGELOG.md` ([0.3.0] section)
- `docs/plan-v0.2.md` (W6 row)
- `docs/roadmap.md` (current status + 0.4.0)
- `docs/session-handoff.md` (this file)
