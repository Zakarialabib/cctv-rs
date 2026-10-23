# cctv-ezviz

The Ezviz driver for `cctv-rs` — the one crate that speaks the Ezviz Open
Platform protocol. It wraps the standalone `ezviz-openapi` client and translates
its responses into the `cctv-core` device model, so vendor wire detail stops at
this boundary and never reaches the `cctv-rs` facade's public API.

- `Driver` — the driver handle: `new`, `client`, `discover`.
- `EzvizClient` — the Ezviz Open API client, re-exported from `ezviz-openapi`.
- `EzvizCamera` — a camera reduced to the device model: id, label, addresses,
  stream URLs, PTZ and AI capability, connectivity.
- `Connectivity` — how the driver reached the camera: `Cloud` or `Direct`.

See [`docs/architecture.md`](../../docs/architecture.md) for the full
architecture.
