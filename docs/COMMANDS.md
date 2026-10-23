# Commands

The camera command inventory. `CommandId` is the **what** a camera is told to do;
the driver crates answer the **how** (protocol, timeouts, retries).

The app never sees a vendor-specific command id. The facade and the reconciler
talk this enum.

## Base device operations (37)

| Category | Commands |
|---|---|
| Discovery | `Discover`, `DiscoverByRange`, `ListUsbCameras`, `LiveSourceList`, `LiveSourceDelete`, `LiveSourceSave`, `LiveSourceGet`, `TestCameraConnection` |
| Stream | `GetCameraFrame`, `StartCameraStream`, `SetGlobalFrameRate`, `SetQualityProfile` |
| Health | `MonitorCameraHealth`, `ScanForNewCameras`, `GetCameraSettings`, `UpdateCameraSettings` |
| PTZ | `PtzMove`, `CameraUpdate`, `CameraSetEnabled`, `CameraSetCritical`, `CameraReorder`, `CameraGetStats`, `CameraGetAllStats`, `CameraTestAll`, `CameraSavePreset`, `CameraGotoPreset`, `CameraDeletePreset`, `CameraScreenshot`, `CameraResetSettings` |
| AI | `CameraGetAvailableModels`, `CameraSetDetectionModel`, `CameraAiDetectionSetEnabled`, `CameraAiDetectionGetEnabled` |
| Zones | `ZoneList`, `ZoneSave`, `ZoneDelete`, `RunZoneDetection` |
| Zone motion | `ZoneMotionGetConfig`, `ZoneMotionSaveConfig`, `ZoneMotionStartMonitor`, `ZoneMotionStopMonitor`, `ZoneMotionListEvents`, `ZoneMotionAcknowledgeEvent` |
| Snapshots | `CameraSaveSnapshot`, `CameraDeleteSnapshot`, `CameraGetSnapshots`, `CameraSnapshotExists` |

## Composite / queue / reconfigure

| | Commands |
|---|---|
| Composite | `CameraStartComposite`, `CameraStopComposite` |
| Queue | `CameraPopCorrectedImage` (frame-queue rework) |
| Reconfiguration | `CameraConfigGet`, `CameraConfigSet`, `CameraGetReconnectionInfo` |

## Result payload — summed over the command

`CommandResultPayload` covers the common shapes: frame, stream handle, stats,
`AllStats`, `Test`, `Presets`, `Settings`, `Config`, `Snapshot` / `Snapshots`,
`MotionEvents`, `Reconfiguration`, `Composite`, `AvailableModels`, `AiStatus`,
`Health`, `Empty`.

## Intent

- **Capability-enforcing.** Calling a command a camera does not implement is a
  typed `CameraError` (`CameraError::NotImplemented`), never `unimplemented!()`.
- **Result-driven.** `CommandResult` carries the payload so the reconciler and
  the facade can act on it without round-trips.
- **State-bearing.** Every command that changes state records it in
  `CameraStateDesired` → `CameraStateCurrent`.
