# Changelog

All notable changes to the `cctv-rs` workspace are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the workspace adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
All six crates are versioned and released together.

## [Unreleased]

### Added

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
