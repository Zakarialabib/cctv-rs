# cctv-rs

The facade over the whole `cctv-rs` workspace, and the only crate the
SignageMaster app (`src/core/cctv.ts`) is allowed to talk to. It fronts
discovery, frames, PTZ, stats and fleet reconciliation behind one handle, using
only the `cctv-core` device model — its public API contains no vendor name.

- `Facade` — the single handle the app holds: `new` and `list_cameras`.
- `CameraHandle` — re-exported from `cctv-core`: the device-model handle the app
  sees in place of a vendor type.
- `CameraError` — re-exported from `cctv-core`: the typed error taxonomy.

See [`docs/architecture.md`](../../docs/architecture.md) for the full
architecture.
