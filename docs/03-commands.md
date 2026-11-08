# 03 — Command Catalogue

> `CommandId` defines *what* a camera is asked to do; drivers implement *how* over
> the wire. The facade and reconciler dispatch commands by enum variant rather
> than by string identifier.

**Read this when** you are adding a command, wiring a driver capability, or
looking up the payload a command returns. The device model behind these variants
is described in [04 — Camera Taxonomy](04-camera-types.md).

Defined in [`crates/cctv-core/src/commands.rs`](../crates/cctv-core/src/commands.rs).

---

## 1. Core Invariants

1. **Deterministic order (`CommandId::order`).** Provides a stable execution
   sequence for fleet reconciliation sweeps, so identical fleets reconcile in
   identical order across scans.
2. **Capability guard (`CommandId::supports`).** Queries whether a driver backend
   supports the requested operation. Unsupported calls fail with
   `CameraError::Unsupported` rather than panicking.

`CommandId` defines **56 variants** across the domain groups below.

---

## 2. Command Groups

| Domain group | Count | Commands |
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

## 3. Command Result Payloads (`CommandResultPayload`)

Results wrap typed data inside the `CommandResultPayload` enum:

| Variant | Encapsulated data |
| --- | --- |
| `Frame` | Decoded frame metadata (`width`, `height`, byte count) |
| `Stream` | Active stream info (`StreamInfo`) |
| `Stats` | Operational online status (`online: bool`) |
| `AllStats` | Snapshot collection (`Vec<CameraSnapshot>`) |
| `Test` | Diagnostic outcome (`passed: bool`, message) |
| `Presets` | Configured camera presets (`Vec<Preset>`) |
| `Settings` | Serialized settings structure |
| `Config` | Typed camera configuration struct (`CameraConfig`) |
| `Snapshot` | Snapshot reference details (path, dimensions) |
| `Snapshots` | Snapshot index list (`Vec<SnapshotRef>`) |
| `MotionEvents` | Timestamped motion detections (`Vec<MotionEvent>`) |
| `Reconfiguration` | Reconnection status structure |
| `Composite` | Composite stream state |
| `AvailableModels` | AI models inventory (`Vec<String>`) |
| `AiStatus` | AI status state (`enabled`, active `model`) |
| `Health` | Health check payload (`healthy`, message) |
| `Empty` | Unit acknowledgement |

---

## See also

- [01 — Architecture](01-architecture.md) §2 — the facade entry points that dispatch these commands.
- [04 — Camera Taxonomy](04-camera-types.md) — the device classes commands act on.

---

## License & Security

- **License:** Apache-2.0 ([LICENSE](../LICENSE)).
- **Security:** see [SECURITY.md](../SECURITY.md) for the vulnerability disclosure
  process.
