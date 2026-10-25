# cctv-core

`cctv-core` is the camera device model — the domain aggregate shared by every
cctv-rs backend. It deliberately contains no tokio, no async runtime, no
protocol or HTTP layer, and no vendor code; only `serde` / `serde_json` for the
command-result value types. That keeps the aggregate embeddable in a Tauri app,
a CLI, or a WASM worker.

Main public items:

- `CameraHandle` — an opaque handle addressing one camera in the fleet.
- `CommandId` — the catalogue of commands a camera understands, with
  `order()` and `supports()` for the reconciler.
- `CommandResult` / `CommandResultPayload` / `StreamHandle` — typed command
  results.
- `CameraState`, `CameraDesired`, `CameraStateCurrent`, `ProbeResult` — the
  state machine and its trust anchor.
- `CameraType` — the camera type taxonomy.
- `CameraError` — the failure taxonomy every driver returns.
- `MockBackend` and `mock_camera()` — the in-memory backend and conformance
  suite.

## Feature `quirks`

Model- and firmware-specific camera defects live in a TOML table rather than in
`match` arms, so a new workaround is a reviewable data change and the table can
ship newer than the crate. It is behind a feature because it pulls a TOML parser
and most builds never read it.

```rust,no_run
use cctv_core::quirks::{QuirkAction, Quirks};

let quirks = Quirks::bundled().expect("the bundled table parses");
if let Some(action) = quirks.action_for("acme", Some("PTZ-9000"), Some("V5.3")) {
    // `start_at_<port>` pins a port; `rtsp_udp_only` drops the interleaved channel.
    let _ = action;
}
```

Two rules, both load-bearing: **order is precedence** (the first matching row
wins, so specific rows go above general ones), and **a row is a claim** (no
source, no row — an unverified quirk fires on devices that never had the defect).
The bundled table ships empty for exactly that reason.

The mock backend plus conformance suite is the bar every driver crate is held
to. See [`docs/architecture.md`](../../docs/architecture.md) for the full design.
