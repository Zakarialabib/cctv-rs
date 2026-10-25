# Changelog

All notable changes to the `cctv-rs` workspace are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the workspace adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
All seven crates are versioned and released together.

## [Unreleased]

### Added

- `cctv-onvif`, the ONVIF driver: SOAP 1.2 request builders and reply parsers for
  the Device, Media and PTZ services, with WS-Security `UsernameToken`
  authentication (`Base64(SHA1(nonce ‖ created ‖ password))`). The sources had
  been committed without a manifest or a crate root, so the crate did not build
  and nothing referenced it; it is now a workspace member with a `lib.rs`, a
  `README.md`, CI coverage and a slot in the publish order.
- `cctv-discovery` transports: the SSDP `M-SEARCH` and WS-Discovery `Probe`
  sweeps. `Probe::scan` performs a real sweep over UDP multicast and
  `Probe::scan_with` runs the same sweep over an injected `Datagram`, so the
  message codecs and the merge / dedupe logic are unit-tested without a network.
  USB enumeration stays the application's concern.
- `cctv-ezviz` driver: `discover` now lists the account's devices and joins
  their channels instead of returning an empty vector, and the account-level
  operations (`alarms`, `capture`) are implemented. `EzvizCameraBackend`
  implements `cctv_core::CameraBackend` for one camera / channel — `probe`,
  `stream`, `ptz` (through the new pure `ptz` direction / speed mapping) and the
  preset add / recall / clear round trip — and passes the conformance suite
  against `cctv-transport`'s fixture HTTP server. The three things the platform
  cannot do (return image bytes, list presets, move to an absolute position) are
  typed `NotImplemented`, not silent failures.
- `cctv-onvif` is now a working driver, not just builders: `OnvifSession` drives
  the SOAP calls over `cctv-transport`'s HTTP client — following (and caching) the
  `XAddr`s `GetCapabilities` returns — and `OnvifCameraBackend` implements
  `CameraBackend` for `probe`, `stream`, `snapshot`, `ptz` and the preset round
  trip. It passes the conformance suite 7/7 with `Expectations::FULL` against a
  canned SOAP server. `stop` names the profile that started moving, because ONVIF
  requires one and `PtzCommand::Stop` does not carry it.
- The facade's registry ships an `onvif` factory beside `ezviz`, so
  `Facade::with_builtin_drivers().attach(config)` can build either.
- `cctv-core` gains the `quirks` table — model- and firmware-specific camera
  defects as **data**, ported from `tv-rs`: a TOML file keyed by
  `(vendor, model prefix, firmware prefix)` with a fixed `action` vocabulary
  (`start_at_<port>`, `rtsp_udp_only`, `auth_not_permitted`, `read_probe_flags`,
  `retry_substream`, `point_user_at_settings`). Order is precedence, and an
  unrecognised action is preserved rather than dropped. Behind the `quirks`
  feature, so a build that only needs the device model pays no TOML parser; the
  bundled table ships **empty** because no defect in this workspace has a source
  yet, and an unverified row is worse than no row.
- `cctv-transport`'s `FrameReader` is implemented — it was a stub that returned
  `None` regardless of input. It now bounds **staleness** (a frame older than the
  budget is dropped rather than delivered late, because a live tile would rather
  show nothing than a stale frame) and the **backlog** (a full queue drops its
  oldest — latest frame wins), counts `delivered` / `dropped` / `starved`
  separately so "no stream" is distinguishable from "stream too slow", and
  **measures** the received rate over a window. It is pure: no I/O, no clock, no
  runtime — the caller stamps frames and supplies "now", so its tests need no
  sleeps.
- `camera_get_stats` reports those measurements instead of hardcoded zeros, and
  `Facade::frame_reader` hands the application the shared reader to push frames
  into. `cctv_rs::clock_ms()` is the clock both sides stamp with. The numbers are
  still zero today because nothing feeds the reader yet — but they are now
  *measured* zero, not a placeholder.
- `cctv-rs` facade: the command surface behind one handle — `get_camera_frame`,
  `start_camera_stream`, `ptz_move`, `camera_config_get` / `camera_config_set`,
  `camera_set_enabled`, the preset round trip, the health / stats / reconnection
  probes, the fleet self-test, and `execute(id, CommandId)` for the
  parameterless catalogue. A driver registry resolves a connection record's
  `driver` key to a factory (`with_builtin_drivers` registers `ezviz`), and
  `discover` / `discover_with` front the LAN sweep.
- `cctv-agent` reconciler: the desired-versus-observed diff and the graded
  remediation it implies (`Grade` / `Action` / `Remediation`), graded from the
  `CameraError` taxonomy — an unreachable camera is retried, an authentication
  failure is reconfigured, a camera the fleet no longer has is disabled, and an
  unknown failure is only notified. `Audit` is a real append-only store.
- `LICENSE` (MIT), `SECURITY.md`, `CONTRIBUTING.md` and the cargo-deny policy that
  the publication path requires.
- Per-crate `README.md` files and publish metadata (`keywords`, `categories`,
  path dependencies carrying a `version`).
- CI (fmt, clippy, tests on Linux and Windows, MSRV, cargo-deny, rustdoc) and an
  ordered release workflow driven by `scripts/publish.sh`.
- The `docs/` set: an index (`docs/README.md`) over `architecture.md`,
  `commands.md`, `camera-types.md`, `roadmap.md`, and the house engineering
  standard.

### Changed

- The facade crate is published as **`cctv-rs`**, not `cctv`: the name `cctv` is
  already taken on crates.io by an unrelated project. In-tree consumers use
  `cctv_rs` where they used `cctv`.
- `scripts/publish.sh` handles the crates.io new-crate rate limit (a burst of 5,
  then 1 every 10 minutes — a first release is eight new crates), refuses to
  start unless `ezviz-openapi` is already published, and hands the token to cargo
  through `CARGO_REGISTRY_TOKEN` instead of `--token` so it never appears in the
  process list.

### Fixed

- `scripts/publish.sh --dry-run` no longer fails when a dependent crate's
  workspace dependency is not on crates.io yet — the state of every dependent on
  the first release. It warns and continues; the real publish waits for the index
  to catch up and skips a crate a previous run already uploaded.
- `cctv-core::CameraState::apply_probe` now records the probe timestamp, so the
  conformance assertion `probe_at.is_some()` holds.
- `CameraHandle` is defined and exported; the command catalogue's `order`,
  `supports` and `default_for` arms cover every declared `CommandId`.

## [0.1.0] - 2026-10-23

### Added

- Initial scaffold: the six-crate workspace — `cctv-core` (device model),
  `cctv-transport` (TLS policy + budgeted frame reader), `cctv-discovery`
  (SSDP/multicast/USB probe), `cctv-ezviz` (Ezviz Open Platform driver),
  `cctv-agent` (fleet reconciler) and `cctv-rs` (the facade).
- `CameraType` taxonomy, `CommandId` catalogue with capability gating, the
  `CameraError` taxonomy, the confidence-carrying camera state machine and the
  mock backend with its conformance suite.

[Unreleased]: https://github.com/Zakarialabib/cctv-rs/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Zakarialabib/cctv-rs/releases/tag/v0.1.0
