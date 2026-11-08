# Changelog

All notable changes to the `cctv-rs` workspace are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the workspace adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
All twelve crates are versioned and released together, plus `ezviz-openapi`
(published from its own repository).

## [0.3.0] - 2026-11-02

### Added
- `cctv-driver`: W6 — vendor-neutral adapter seam (DriverAdapter + DriverRegistry).
- RTSP session reconnection hardening: bounded exponential backoff
  (RECONNECT_MAX_DELAY 10 s, RECONNECT_MAX_RETRIES 8), StreamEnd::Stalled
  classification, health-state reporting on reconnect failure.
- Live stream health watchdog: cctv_core health module (Health/Watchdog),
  wired into the driver stream path through READ_STALL and the reconnect
  policy.

## [Unreleased]

### Added

- **`cctv-pair` crate** (new): device pairing ceremony and trust store —
  `Ceremony` orchestrates the AKE handshake, `TrustStore` persists trusted
  device keys, `Uri` parses camera URIs with auth, and `zbase32` encoding for
  human-comparable fingerprints. Integration tests cover the full ceremony flow.
- **QUIC transport** (`cctv-transport`): QUIC-based transport via `iroh` —
  `QuicEndpoint` provides encrypted, multiplexed streams for camera feeds with
  automatic connection management and migration.
- **Annex-B elementary-stream probe** (`cctv-rtp`): `AnnexBProbe` extracts
  NAL units from RTP payloads, building `MediaDescriptor` with codec profile,
  level, and SPS/PPS/VPS parsing. Validates H.264/H.265 bitstream structure
  without full decode.
- **Sender pacing & drop-to-keyframe** (`cctv-transport`): `Pacer` controls
  RTP packet emission timing with token-bucket pacing; `DropToKeyframe` policy
  flushes the GOP on congestion, forcing a clean IDR resume.
- **Capability matrix & hello.update** (`cctv-core`): `CapabilityMatrix`
  advertises supported features (PTZ, presets, analytics) in the ONVIF hello
  handshake; `hello.update` handles capability changes at runtime.
- `cctv-core`'s bundled quirks table ships **three sourced rows** — Hikvision's
  ONVIF-disabled default (V5.5.0), Ezviz's disabled local service / image
  encryption, and Sony SRG-360's absent RTSP interleaved TCP bit stream.
- `xtask`, the workspace task runner: `cargo xtask publish` and `cargo xtask hw-test`.

### Changed

- **Driver consolidation — the published surface drops from fifteen crates to
  twelve.** `cctv-discovery`, `cctv-ezviz`, `cctv-stream-uvc` and `cctv-onvif`
  are folded into `cctv-driver` behind cargo features (`discovery` and `onvif` —
  both default, `ezviz`, `uvc`, `uvc-ptp`, `ddc`, plus `driver-mock`/`mobile`),
  and the seam is split into a `DriverAdapter` factory surface and the
  `CameraBackend` live handle. `cctv-onvif` was a deprecated re-export
  shim for one release. Publish order and CI are updated accordingly; existing
  code moves to `cctv_driver::{discovery, onvif, ezviz, uvc}`.
- **Graded reconciliation policies** (`cctv-agent`): `Policy` / `PolicyTracker` /
  `PolicyVerdict` gate every remediation and write an audit entry.
- **The workspace is licensed under Apache-2.0**, replacing MIT.
- `rust-toolchain.toml` pins the `stable` channel instead of a fixed compiler.
- `cctv-rag` joins the release plumbing.

### Fixed

- **PTZ watchdog virtual clock** (`cctv-agent`): the watchdog now drives from
  the tokio clock, not wall-clock time, so tests and skewed-clock cameras
  behave identically.

## [0.2.0] - 2026-11-01

### Added

- **StreamMux & SnapshotProxy** (PR #38, Phase 5): `cctv_agent::StreamMux` is a
  GOP-cached NALU multiplexer for RTSP sub-streams — pulls the sub-stream once,
  caches `[SPS, PPS, (VPS), IDR]`, and broadcasts NAL units so a client joining
  `GET /agent/cameras/{id}/live_ws` renders frame 1 with zero GOP wait.
  `SnapshotProxy` proxies native snapshot retrieval with RTSP Digest auth behind
  a 500 ms cache, so concurrent requests neither consume camera RTSP sessions
  nor burn host CPU. `cctv-onvif` now exports its PTZ SOAP builders/parsers
  (`build_*` / `parse_*`) for the watchdog, and `cctv-agent` re-exports the
  provisioning and session-budget types (`SessionManager`, `CameraCapabilities`,
  `DecryptedCameraAuth`, …).
- **Skewed-clock WS-Security & RTSP Digest** (PR #36): `cctv_onvif::auth`
  computes `<wsu:Created>` and the `#PasswordDigest` from host UTC plus the
  observed camera-clock offset, so devices with drifted clocks or wrong
  timezones authenticate; PTZ requests use the same auth. `cctv_rtsp::digest`
  is a full RTSP Digest state machine — parses `401 WWW-Authenticate: Digest`
  challenges (RFC 2617 and RFC 7616, `qop="auth"` included) and generates the
  `Authorization` header, pulling the password from `DecryptedCameraAuth`'s
  zero-on-drop `Zeroizing<String>` (`zeroize` deduplicated across the graph).
- **`cctv-detect` wired into the facade** (it was an orphan — nothing depended
  on it): `Facade` gains the zone surface — `zone_save` / `zone_list` /
  `zone_config` / `zone_delete` for `ZoneConfig`, and `record_detection`, which
  gates a `Detection` on the zone's own monitor settings (unknown zone = typed
  error, below `threshold` = not reported, `min_detections` inside `window_s`
  before the monitor fires) and emits a `MotionEvent` stamped on the facade
  clock with the window cleared.  `ZoneMotionListEvents` now answers through
  `Facade::execute` with the `MotionEvents` payload.  Types re-exported from
  `cctv_rs` (`Detection`, `Zone`, `ZoneConfig`, `Region`, `RegionKind`).
  Publishing fixed for the crate: `README.md` added (the manifest declared it
  but the file did not exist — `cargo package` would have failed), and it takes
  its place in the workspace dependency table, the `Cargo.toml` member order,
  `scripts/publish.sh` ORDER (before the facade) and CI's per-crate job.
  The facade's dry run now covers 11 crates.
- `cctv_agent::CommandRelay` and `Remediation::commands` (plan v0.2 **W5** —
  active remediation): a remediation now *names* the `CommandId`s it
  authorizes (`MarkOffline` → `TestCameraConnection`, `TakeOutOfService` →
  `CameraSetEnabled`, `Advance(Streaming|Recording)` → `StartCameraStream`,
  `Advance(Retrying)` → `TestCameraConnection`; `Hold` and non-actionable
  grades issue nothing — an auth failure never commands, only an operator can
  supply credentials).  `Reconciler::with_command_relay` makes issuance
  active: commands flow through the relay as they are graded, each audited as
  `issued <CommandId>`, and without a relay the reconciler stays report-only.
  The relay is sync by contract (`Send + Debug`) — reconciler issuance is
  I/O-free (the runner and stream modules own connections), modeled on
  `tv-rs`'s `with_command_relay`.
  Gate: `cargo test -p cctv-agent` (25 passed, 5 new).
- `cctv-onvif::discovery` (plan v0.2 **W4**): the `Discoverable` trait and
  `OnvifDiscovery`, a WS-Discovery sweep delegated to `cctv-discovery` — same
  `Datagram` seam, envelope, parser and identity converter — gated by ONVIF
  evidence (`onvif://` scope or an ONVIF `Types` entry) and by an addressable
  HTTP(S) `XAddr`, because without the control-service URL there is no
  [`OnvifSession`] to open.  WS-Discovery only: the ONVIF Core Specification
  requires every device to answer a `Probe`, so SSDP's UPnP noise stays out of
  the ONVIF crate (the merged sweep remains in `cctv-discovery`).  Publish
  order updated: `cctv-discovery` now precedes `cctv-onvif`.
  Gate: `cargo test -p cctv-onvif` (34 passed).
- `cctv-detect`, the detection device model: `Detection` (motion or object, with
  class, clamped confidence and an optional bounding box), `Zone` / `RegionKind`
  (`point` in native pixels or `rectangle` normalised to 0.0..1.0, each field
  clamped on its own) and `ZoneConfig` (threshold, minimum detections, window).
  Free of I/O by design — the driver owns the frame, the reconciler owns the
  response.  Workspace member; not yet referenced by another crate and not yet
  in the publish order.
- `CameraState` retry tracking: `retry_count` / `retry_after` fields, the
  `Retrying` desired and current states, and `CameraState::retry` to enter
  recovery.  A successful probe resets the counters and withdraws the retrying
  intent; the agent's reconciler grades `Retrying` by stepping toward the
  retrying state rather than panicking on the unhandled variant.
- `cctv-transport`'s `net` feature: the raw socket layer (plan v0.2 W2). TCP,
  UDP and TLS over `tokio` + `rustls`, with the crate's existing `Policy`
  applied as data — minimum version, pinned certificates, ALPN — rather than a
  second, parallel policy type. `Stream` is an enum (plain TCP or a `rustls`
  session) so a driver reads both the same way, and `read_chunks` /
  `feed_chunks` push byte counts into the pure `FrameReader`, whose signature is
  unchanged. The feature is off by default, pulls no `reqwest`, and no
  `native-tls` is linked anywhere in the workspace. `accept_invalid_certs` is a
  real opt-in: the integration test starts a TLS server with a self-signed
  certificate generated at test time by `rcgen` and asserts the handshake
  succeeds with the flag and fails without it.
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
- `cctv-onvif` became a working driver, not just builders: `OnvifSession` drives
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
- `cctv-core` is **`alloc`-only**, and the JSON DOM has left the contract. What
  blocked `no_std` was not `std` — it was `serde_json::Value` sitting inside the
  public API: `Preset::config`, `CameraConfig::extra`, and a
  `CommandResultPayload::Config { config: Value }` that serialised a typed struct
  to a DOM and handed the DOM over. Nothing read any of them, so they are deleted
  rather than feature-gated, and the `Config` payload now carries the
  `CameraConfig` itself. `serde_json` moves to dev-dependencies, where the tests
  that exercise the wire format still use it.
- Two things stayed `std` and became features instead of pretending otherwise:
  `testing` (default) is the mock backend, which needs `Mutex` for interior
  mutability because a `CameraBackend` must be `Sync`, and `alloc` has no mutex;
  `quirks` is the TOML table, because `toml` is std-only. So
  `cargo check -p cctv-core --no-default-features` is the alloc-only build, and
  CI now runs it — the dependency graph there is `serde` and nothing else.
- `CameraState`'s clock is `std`-gated for the same reason: `core` has no
  `SystemTime`, so an alloc-only build stamps transitions with `0` and a caller
  that needs real timestamps records them itself — which `cctv-agent`'s audit
  trail already does.
- `cctv-rtp` — a new crate, and the payload half of the stream path: the RTP
  header (CSRC lists, extensions, padding) and H.264 / H.265 access-unit
  reassembly across single NALs, STAP-A / AP and FU-A / FU. **Dependency-free**,
  because it knows nothing about RTSP or SDP — the `base64` that
  `sprop-parameter-sets` needs belongs to the session, not the payload. 34 tests
  pin what breaks: absurd padding is rejected rather than slicing out of range, a
  truncated aggregate yields no unit rather than a partial one, and the two-byte
  HEVC header is rebuilt from the FU indicator plus the fragment header's 6-bit
  type.
- `cctv-rtsp` — narrowed to the **session** half: RTSP request/response framing
  (including the `$`-prefixed interleaved media a TCP transport multiplexes onto
  the same connection) and SDP (tracks, payload types, `a=control`, out-of-band
  SPS/PPS). Still pure — byte slices in, no socket, no runtime. 19 tests, mostly
  about the incremental parser: a truncated buffer is `Incomplete` rather than
  `Malformed`, and an `a=rtpmap` for PT 97 is not applied to a track that named
  96.
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
- `LICENSE` (Apache-2.0), `SECURITY.md`, `CONTRIBUTING.md` and the cargo-deny policy that
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
  then 1 every 10 minutes — a first release is twelve new crates), refuses to
  start unless `ezviz-openapi` is already published, and hands the token to cargo
  through `CARGO_REGISTRY_TOKEN` instead of `--token` so it never appears in the
  process list.

### Fixed

- The workspace builds again: three commits (`2e032a6`, `a31c4aa`) had left
  `cctv-core`, `cctv-agent`, `cctv-detect` and the RTSP fixture test broken —
  tests stranded inside `impl CameraState`, the fixture's `respond` function
  body spliced into `serve`, an unhandled `CameraDesired::Retrying` match arm,
  `cctv-detect` tests written against an API that was never committed, and two
  retry-tracking logic bugs.  `cargo fmt --check`, `clippy -D warnings`,
  `cargo test --workspace`, the `quirks` / `net` feature gates, the
  alloc-only and tokio-free contract gates, `cargo doc -D warnings` and
  `publish.sh --dry-run` all pass again.
- `CameraState::retry` now records the caller's `retry_after` verbatim instead
  of silently raising it to the wall clock (the state machine has no clock of
  its own in an `alloc`-only build, where the old `max` was a no-op anyway).
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

[Unreleased]: https://github.com/Zakarialabib/cctv-rs/compare/v0.3.0...HEAD
[0.2.0]: https://github.com/Zakarialabib/cctv-rs/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Zakarialabib/cctv-rs/releases/tag/v0.1.0
