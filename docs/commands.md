# Command Catalogue

`CommandId` defines *what* an IP camera is asked to do. Driver crates implement *how* the operation is performed over the wire. The facade and reconciler address operations by enum values — eliminating string-based command dispatch.

Source code reference: [`crates/cctv-core/src/commands.rs`](../crates/cctv-core/src/commands.rs).

## Key Invariants

1. **Deterministic Execution Order (`CommandId::order`):** Fixes a stable coarse-to-fine sweep sequence for fleet reconciliation.
2. **Typed Capability Guard (`CommandId::supports`):** Queries whether a camera driver supports an operation. Unsupported commands return `CameraError::Unsupported` rather than panicking.

`CommandId` defines **56 variants** organized into distinct domain groups.

---

## Catalogue Breakdown

| Group | Count | Enum Variants |
| --- | --- | --- |
| Discovery | 3 | `Discover`, `DiscoverByRange`, `ListUsbCameras` |
| Live Source | 4 | `LiveSourceList`, `LiveSourceDelete`, `LiveSourceSave`, `LiveSourceGet` |
| Stream Controls | 4 | `GetCameraFrame`, `StartCameraStream`, `SetGlobalFrameRate`, `SetQualityProfile` |
| Health Monitoring | 3 | `TestCameraConnection`, `MonitorCameraHealth`, `ScanForNewCameras` |
| Configuration | 5 | `GetCameraSettings`, `UpdateCameraSettings`, `CameraResetSettings`, `CameraConfigGet`, `CameraConfigSet` |
| PTZ Controls | 5 | `PtzMove`, `CameraUpdate`, `CameraSetEnabled`, `CameraSetCritical`, `CameraReorder` |
| Fleet Diagnostic | 3 | `CameraGetStats`, `CameraGetAllStats`, `CameraTestAll` |
| Presets | 3 | `CameraSavePreset`, `CameraGotoPreset`, `CameraDeletePreset` |
| Snapshots | 5 | `CameraScreenshot`, `CameraSaveSnapshot`, `CameraDeleteSnapshot`, `CameraGetSnapshots`, `CameraSnapshotExists` |
| Detection & Zones | 10 | `DetectObjects`, `ZoneList`, `ZoneSave`, `ZoneDelete`, `RunZoneDetection`, `ZoneMotionGetConfig`, `ZoneMotionSaveConfig`, `ZoneMotionStartMonitor`, `ZoneMotionStopMonitor`, `ZoneMotionListEvents` |
| Reconnection | 1 | `CameraGetReconnectionInfo` |
| Image Pipeline | 3 | `CameraPopCorrectedImage`, `CameraStartComposite`, `CameraStopComposite` |
| AI Engine | 4 | `CameraGetAvailableModels`, `CameraSetDetectionModel`, `CameraAiDetectionSetEnabled`, `CameraAiDetectionGetEnabled` |
| Stream Monitors | 2 | `CameraGetMonitorHealth`, `CameraGetMonitorStreamHealth` |

---

## Command Result Payloads (`CommandResultPayload`)

Every `CommandId` maps directly to a typed `CommandResultPayload` variant:

| Result Payload | Carried Data |
| --- | --- |
| `Frame` | Decoded frame metadata: `width`, `height`, byte count |
| `Stream` | Active stream info: `handle`, stream URI / details |
| `Stats` | Operational state: `online` |
| `AllStats` | Snapshot inventory: `Vec<CameraSnapshot>` |
| `Test` | Connection diagnostic result: `passed`, detail message |
| `Presets` | Configured camera presets: `Vec<Preset>` |
| `Settings` | Serialized setting value structure |
| `Config` | Typed camera configuration struct (`CameraConfig`) |
| `Snapshot` | Captured file path, image width & height |
| `Snapshots` | Snapshot index: `Vec<SnapshotRef>` |
| `MotionEvents` | Detected motion log: `Vec<MotionEvent>` |
| `Reconfiguration` | Reconnection information payload |
| `Composite` | Composite stream active state |
| `AvailableModels` | AI models list: `Vec<String>` |
| `AiStatus` | AI status: `enabled`, active `model` |
| `Health` | Health check payload: `healthy`, status message |
| `Empty` | Unit acknowledgement |
