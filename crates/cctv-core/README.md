# cctv-core

The device model for `cctv-rs` — the one type everything else leans on. Used by
the app, the reconciler and every driver, so it keeps no tokio, no async runtime,
no protocol layer and no vendor code in it. Only `serde` / `serde_json` for the
command-result value types, so the aggregate embeds in a Tauri app, a CLI or a
WASM worker.

## You get

- `CameraHandle` — an opaque handle addressing one camera in the fleet.
- `CommandId` — the command catalogue, with `order()` and `supports()` for the
  reconciler.
- `CommandResult` / `CommandResultPayload` / `StreamHandle` — typed command
  results.
- `CameraState`, `CameraDesired`, `CameraStateCurrent`, `ProbeResult` — the state
  machine and its trust anchor.
- `CameraType` — the camera-type taxonomy.
- `CameraError` — the failure taxonomy every driver returns.
- `MockBackend` / `mock_camera()` / the conformance suite — the bar every driver
  is held to.

## Feature `quirks`

Model- and firmware-specific defects live in a TOML table instead of `match`
arms, so a new workaround is a reviewable data change and the table can ship
newer than the crate. It is behind a feature because it pulls a TOML parser and
most builds never read it.

```rust,no_run
use cctv_core::quirks::{QuirkAction, Quirks};

let quirks = Quirks::bundled().expect("the bundled table parses");
if let Some(action) = quirks.action_for("acme", Some("PTZ-9000"), Some("V5.3")) {
    // `start_at_<port>` pins a port; `rtsp_udp_only` drops the interleaved channel.
}
```

Two rules, both load-bearing: **order is precedence** (first matching row wins)
and **a row is a claim** (no source, no row — an unverified quirk fires on
devices that never had the defect). The bundled table ships empty for exactly
that reason.

See `docs/architecture.md` for the design.
