# cctv-ezviz

The Ezviz driver for `cctv-rs` — the one crate that speaks the Ezviz Open
Platform protocol. It wraps the standalone `ezviz-openapi` client and translates
its responses into the `cctv-core` device model, so vendor wire detail stops at
this boundary and never reaches the `cctv-rs` facade's public API.

- `Driver` — the account-level handle: `discover`, `alarms`, `capture`.
- `EzvizCameraBackend` — one camera / channel, as a `cctv_core::CameraBackend`;
  the subject of the conformance suite (`probe`, `stream`, `ptz`, presets).
- `EzvizCamera` / `Connectivity` / `Alarm` — the device-model shapes the driver
  hands back.
- `ptz` — the pure translation of a `cctv_core::PtzCommand` into the platform's
  discrete direction + speed vocabulary.
- `DriverError` — the driver's failure type, and its mapping onto
  `cctv_core::CameraError`.
- `EzvizClient` — the vendor client, re-exported from `ezviz-openapi` for
  callers that need an endpoint the driver does not model.

## What the platform cannot do

Three gaps are typed errors rather than silent failures, and the conformance
expectations name them:

- **Snapshots.** `/device/capture` only *requests* a picture; the platform
  delivers it out of band through its picture service, so the driver cannot
  return image bytes. `snapshot()` stays at the trait default
  (`NotImplemented`).
- **Listing presets.** The platform can add, recall and clear a preset but has no
  list endpoint, so `presets()` is `NotImplemented`. `save_preset` / `goto_preset`
  / `delete_preset` work; a preset id is the platform's numeric index.
- **Absolute PTZ.** Moves are eight compass directions plus zoom, at three
  speeds. `PtzCommand::Continuous` is mapped onto them (`ptz`), and
  `PtzCommand::Absolute` is `NotImplemented`.

Play addresses expire, so `discover` does not cache a stream URL — `stream()`
resolves one on demand.

## Tests

`tests/mock_server.rs` runs the driver against `cctv-transport`'s fixture HTTP
server: no camera, no credentials, no network. The token response names the
fixture as the region's `areaDomain` via the `{{base}}` placeholder, so the
client's region switch is exercised for real.

See [`docs/architecture.md`](../../docs/architecture.md) for the full
architecture.
