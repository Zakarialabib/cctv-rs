# Roadmap

What is built, what is next, and what is deliberately deferred.

## Phases

| # | Deliverable | Gate | Status |
| --- | --- | --- | --- |
| 1 | `cctv-core`: the device model — traits, state machine, error taxonomy, mock backend, conformance suite, `quirks` | `cargo check -p cctv-core` clean, **no tokio in its graph** | landed — conformance suite green |
| 2 | `cctv-transport` (TLS policy + budgeted frame reader) and `cctv-discovery` (SSDP / multicast / USB) | both compile | crates landed and compile; the discovery transports are not wired yet — `Probe::scan` finds nothing |
| 3 | `cctv-ezviz`: wrap `ezviz-openapi` — token, device list, stream, PTZ, alarms, capture | `cargo test -p cctv-ezviz` against a mock server | driver surface landed; mock-server tests pending |
| 4 | `cctv-rs` facade, and rewriting the application's camera surface onto it | `cargo check -p cctv-rs` | facade handle landed (`new`, `list_cameras`); the application-side rewrite is not started |
| 5 | `cctv-agent`: the reconciler — desired state → graded remediation, audit trail | `cargo test -p cctv-agent` | crate landed; reconciler logic pending |
| 6 | Integration *with* the application — no vendor vocabulary left in it | the application surface contains no vendor name | not started |

Phases 1–4 are the path to a working driver. Phase 5 is post-V1, and phase 6 is
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
   published separately; `cctv-ezviz` consumes it through a path dependency and
   never copies vendor code.
4. **Vendor range.** `cctv-rs` owns Ezviz, Hikvision, Dahua and others — **one
   driver crate per vendor**, and a driver crate is protocol only.
5. **Publish scope.** `cctv-core` ships the device model now: traits,
   `CameraHandle`, `CameraError`, the conformance suite. That is the public
   contract.
6. **App facade.** The application's camera module becomes a pure facade over the
   workspace; no vendor name survives in it.
7. **Facade package name.** The facade publishes as **`cctv-rs`**, because `cctv`
   is already taken on crates.io by an unrelated project.

## Open items

- Wire the discovery transports (`Probe::scan` currently returns an empty sweep).
- Mock-server tests for the Ezviz driver.
- The `quirks` remedy map: the data model is pure and returns `None`; a concrete
  vendor supplies its map from data — no vendor appears in the code.
- Lock down the reverse-engineered Ezviz surface before writing more than passive
  reads into the driver.
- The application-side rewrite in phase 4 and the integration in phase 6.

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
