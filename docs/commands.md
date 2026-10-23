# Commands

`CommandId` is the *what* a camera is asked to do. The driver crates answer the
*how*. The facade and the reconciler address commands **by value, never by
string**, so the application never sees a vendor-specific command id.

Source of truth: [`crates/cctv-core/src/commands.rs`](../crates/cctv-core/src/commands.rs).

## Two invariants

- **`CommandId::order`** gives the reconciler a deterministic sweep order (coarse
  to fine). Declaration order matches `order()`, so the reconciler's pass is
  readable from the top of the enum down.
- **`CommandId::supports`** is the capability gate. A command a camera cannot
  perform is a typed `CameraError` — never `unimplemented!()`.

`CommandId` has **56 variants**. They are grouped below exactly as the source
groups them.

## Catalogue

| Group | Count | Variants |
| --- | --- | --- |
| discovery | 3 | `Discover`, `DiscoverByRange`, `ListUsbCameras` |
| live source | 4 | `LiveSourceList`, `LiveSourceDelete`, `LiveSourceSave`, `LiveSourceGet` |
| health | 1 | `TestCameraConnection` |
| stream | 4 | `GetCameraFrame`, `StartCameraStream`, `SetGlobalFrameRate`, `SetQualityProfile` |
| health | 2 | `MonitorCameraHealth`, `ScanForNewCameras` |
| settings | 2 | `GetCameraSettings`, `UpdateCameraSettings` |
| objects | 1 | `DetectObjects` |
| PTZ | 5 | `PtzMove`, `CameraUpdate`, `CameraSetEnabled`, `CameraSetCritical`, `CameraReorder` |
| stats | 3 | `CameraGetStats`, `CameraGetAllStats`, `CameraTestAll` |
| presets | 3 | `CameraSavePreset`, `CameraGotoPreset`, `CameraDeletePreset` |
| snapshots | 1 | `CameraScreenshot` |
| settings | 1 | `CameraResetSettings` |
| zones | 4 | `ZoneList`, `ZoneSave`, `ZoneDelete`, `RunZoneDetection` |
| zone motion | 6 | `ZoneMotionGetConfig`, `ZoneMotionSaveConfig`, `ZoneMotionStartMonitor`, `ZoneMotionStopMonitor`, `ZoneMotionListEvents`, `ZoneMotionAcknowledgeEvent` |
| snapshots | 4 | `CameraSaveSnapshot`, `CameraDeleteSnapshot`, `CameraGetSnapshots`, `CameraSnapshotExists` |
| reconnection | 1 | `CameraGetReconnectionInfo` |
| config | 2 | `CameraConfigGet`, `CameraConfigSet` |
| queue | 1 | `CameraPopCorrectedImage` |
| composite | 2 | `CameraStartComposite`, `CameraStopComposite` |
| AI models | 4 | `CameraGetAvailableModels`, `CameraSetDetectionModel`, `CameraAiDetectionSetEnabled`, `CameraAiDetectionGetEnabled` |
| monitors | 2 | `CameraGetMonitorHealth`, `CameraGetMonitorStreamHealth` |

The `health` and `settings` headings each appear twice, and `snapshots` twice,
because the catalogue is grouped by *concern within declaration order*, not by a
single contiguous block. That is deliberate: the enum is ordered for the
reconciler's sweep, and the sections label what each run of variants is for.

## Result payloads

`CommandResultPayload` closes the loop: each `CommandId` maps to exactly one
result shape, so the app never guesses the concrete variant.
`CommandResultPayload::default_for(command)` is the "wire" — the zero value a
command starts from. There are **17** payloads.

| Payload | Carries |
| --- | --- |
| `Frame` | one decoded frame: `width`, `height`, `bytes` |
| `Stream` | a started stream: `handle`, `stream` |
| `Stats` | `fps`, `bytes_per_sec`, `online` |
| `AllStats` | `cameras: Vec<CameraSnapshot>` |
| `Test` | `passed`, `detail` |
| `Presets` | `presets: Vec<Preset>` |
| `Settings` | `settings: serde_json::Value` |
| `Config` | `config: serde_json::Value` |
| `Snapshot` | `path`, `width`, `height` |
| `Snapshots` | `snapshots: Vec<SnapshotRef>` |
| `MotionEvents` | `events: Vec<MotionEvent>` |
| `Reconfiguration` | `info` |
| `Composite` | `started` |
| `AvailableModels` | `models: Vec<String>` |
| `AiStatus` | `enabled`, `model: Option<String>` |
| `Health` | `healthy`, `detail` |
| `Empty` | nothing |

## Supporting types

- `CommandResult` — a payload plus its `CommandId`, optionally tagged with the
  `camera_id` it came from (`CommandResult::new`, `CommandResult::for_camera`).
- `CameraSnapshot`, `MotionEvent`, `Preset`, `SnapshotRef` — the value types the
  payloads carry.
- `CameraHandle` — an opaque handle addressing one camera in the fleet.
- `StreamHandle` — an opaque handle addressing one started stream.

---

**License:** MIT — see [`LICENSE`](../LICENSE).
**Security:** see [`SECURITY.md`](../SECURITY.md).
