# Roadmap

What is built, what is next, and what is deliberately deferred.

## Phases

| # | Deliverable | Gate | Status |
| --- | --- | --- | --- |
| 1 | `cctv-core`: the device model — traits, state machine, error taxonomy, mock backend, conformance suite, `quirks` | `cargo check -p cctv-core` clean, **no tokio in its graph** | landed — conformance suite green; the `quirks` table loader is behind the `quirks` feature and ships with no rows (see open items) |
| 2 | `cctv-transport` (TLS policy + budgeted frame reader) and `cctv-discovery` (SSDP + WS-Discovery) | both compile | landed — the SSDP `M-SEARCH` and WS-Discovery `Probe` sweeps run over UDP multicast, with the codecs and the merge / dedupe logic unit-tested behind the `Datagram` seam |
| 3 | `cctv-onvif`: the ONVIF driver — Device / Media / PTZ SOAP builders and parsers, WS-Security `UsernameToken` | `cargo test -p cctv-onvif` | landed — `OnvifSession` drives the builders over `cctv-transport`'s HTTP client (following the `XAddr`s `GetCapabilities` returns, cached), and `OnvifCameraBackend` passes the conformance suite 7/7 against a canned SOAP server |
| 4 | `cctv-ezviz`: wrap `ezviz-openapi` — token, device list, stream, PTZ, alarms, capture | `cargo test -p cctv-ezviz` against a mock server | landed — the conformance suite and the account-level operations run against `cctv-transport`'s fixture server; the three platform gaps (image bytes, listing presets, absolute PTZ) are typed errors |
| 5 | `cctv-rs` facade, and rewriting the application's camera surface onto it | `cargo check -p cctv-rs` | facade landed — the command surface, the driver registry, the LAN sweep and the fleet reconciliation are implemented and tested; the application-side rewrite is not started |
| 6 | `cctv-agent`: the reconciler — desired state → graded remediation, audit trail | `cargo test -p cctv-agent` | landed — graded remediation from the error taxonomy, an append-only audit trail, and the fleet sweep |
| 7 | Integration *with* the application — no vendor vocabulary left in it | the application surface contains no vendor name | not started |

Phases 1–5 are the path to a working driver. Phase 6 is post-V1, and phase 7 is
the point at which the camera half of the refactor is finished.

## Deliberately out of scope

- **PTZ protocol negotiation against live hardware.** Driver work, deferred past
  V1.
- **Serving live streams.** The streaming service is a separate, planned
  repository. `cctv-transport` reads and buffers frames; it is not a media server.
- **A terminal front end.** Named in the original design notes; not part of the
  published workspace, so nothing here defines its API.

## Locked decisions

These were confirmed on 2026-10-23 and are not open for relitigating without a
reason.

1. **Repo.** `cctv-rs` is a standalone workspace, published on its own.
2. **Layout.** One crate per concern under `crates/`, listed explicitly in the
   root manifest — a glob that stops matching fails the build with a confusing
   error, and the member list doubles as the publish order.
3. **Ezviz.** The Ezviz Open Platform client is **one crate** here
   (`cctv-ezviz`). The standalone `ezviz-openapi-rs` repository is kept and
   published separately; `cctv-ezviz` never copies vendor code. The workspace
   consumes it as a **git dependency pinned to a rev for development; published
   crates depend on the crates.io release** (the manifest carries both `git` +
   `rev` and `version`, and `cargo publish` keeps only the latter). Owner
   decision, 2026-10-23.
4. **Vendor range.** `cctv-rs` owns Ezviz, Hikvision, Dahua and others — **one
   driver crate per vendor**, and a driver crate is protocol only.
5. **ONVIF.** ONVIF is a *protocol*, not a vendor: it gets its own driver crate
   (`cctv-onvif`) that every ONVIF vendor shares, the same way `cctv-transport`
   carries the wire helpers all drivers share.
6. **Publish scope.** `cctv-core` ships the device model now: traits,
   `CameraHandle`, `CameraError`, the conformance suite. That is the public
   contract.
7. **App facade.** The application's camera module becomes a pure facade over the
   workspace; no vendor name survives in it.
8. **Facade package name.** The facade publishes as **`cctv-rs`**, because `cctv`
   is already taken on crates.io by an unrelated project.

## Open items

- **Nothing pushes frames yet.** `FrameReader` is real and `camera_get_stats`
  measures from it, so the numbers are no longer hardcoded — but no RTSP client
  feeds the reader, so they read zero until the application's stream consumer
  takes `Facade::frame_reader(id)` and pushes. That consumer (RTSP
  DESCRIBE / SETUP / PLAY plus RTP depacketisation) is the remaining piece.
- The `quirks` table ships **empty**. The loader, the action vocabulary and the
  precedence rules are implemented and tested (`cctv-core`, feature `quirks`), but
  no camera defect in this workspace has a source yet — and a row without one is a
  guess that fires on devices that never had the defect. Rows land in
  `crates/cctv-core/quirks/default.toml` as they are confirmed against hardware.
- Lock down the reverse-engineered Ezviz surface before writing more than passive
  reads into the driver.
- The application-side rewrite in phase 5 and the integration in phase 7.

## Acceptance criteria

- `cargo check --workspace` exits 0.
- `cargo tree -p cctv-core` shows no tokio in its dependency graph.
- The conformance suite passes against the mock backend.
- The facade exposes the commands the application depends on —
  `get_camera_frame`, `start_camera_stream`, `ptz_move`,
  `camera_config_get` / `camera_config_set`, `camera_get_presets` /
  `camera_save_preset` / `camera_goto_preset` / `camera_delete_preset`,
  `camera_get_reconnection_info` — and types them end to end.
- No vendor name appears outside the driver crate that speaks it.
