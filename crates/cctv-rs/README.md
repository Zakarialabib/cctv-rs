# cctv-rs

The facade over the whole `cctv-rs` workspace, and the only crate the
SignageMaster app (`src/core/cctv.ts`) is allowed to talk to. It fronts
discovery, frames, PTZ, stats and fleet reconciliation behind one handle, using
only the `cctv-core` device model — its public API contains no vendor name.

- `Facade` — the single handle the app holds:
  - registry: `register`, `attach`, `unregister`, `list_cameras`, `config`,
  - commands: `get_camera_frame`, `start_camera_stream`, `ptz_move`,
    `camera_config_get` / `camera_config_set`, `camera_set_enabled`,
    `camera_get_presets` / `save` / `goto` / `delete`,
    `test_camera_connection`, `monitor_camera_health`, `camera_get_stats`,
    `camera_get_reconnection_info`, `camera_test_all`, `camera_get_all_stats`,
  - the catalogue entry point `execute(id, CommandId)`,
  - fleet: `reconcile`, and `discover` / `discover_with` for the LAN sweep.
  - frames: `frame_reader(id)` — the shared `FrameReader` the application pushes
    frames into; `camera_get_stats` measures from it. `clock_ms()` is the clock
    both sides stamp with.
- `DriverFactory` — how a connection record's `driver` key becomes a live
  backend, so no vendor name is part of this API. `Facade::with_builtin_drivers`
  registers the ones the workspace ships (`ezviz`, `onvif`).
- `Remediation` / `Grade` / `Action` / `Audit` — re-exported from `cctv-agent`:
  what the reconciler decided, and why.

## Return types

Two shapes, on purpose:

- A command with a device-model equivalent returns **it** — `get_camera_frame`
  gives back a `Snapshot` (the image bytes; the catalogue's `Frame` payload
  carries only dimensions and a length, so the image cannot travel in it),
  `start_camera_stream` a `StreamInfo`, `camera_get_presets` the presets.
- A command whose result has no device-model equivalent returns a
  `CommandResult` carrying the `CommandResultPayload` variant that names it —
  health probes, stats, reconnection info, the fleet self-test.

## Known zeros

`camera_get_stats` reports `fps: 0.0` and `bytes_per_sec: 0` today: nothing
measures them until `cctv-transport`'s frame reader is wired into the stream
path (see [`docs/roadmap.md`](../../docs/roadmap.md)). Only `online` is measured.

`discover` blocks for the probe's collection window, so it belongs on a worker
thread; `discover_with` takes an injected transport.

See [`docs/architecture.md`](../../docs/architecture.md) for the full
architecture.
