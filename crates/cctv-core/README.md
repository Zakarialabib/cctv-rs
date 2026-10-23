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

The mock backend plus conformance suite is the bar every driver crate is held
to. See [`docs/architecture.md`](../../docs/architecture.md) for the full design.
