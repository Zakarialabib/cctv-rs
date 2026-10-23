//! Command inventory for a camera device.
//!
//! `CommandId` is the *what*; the driver crates answer the *how*.  The facade
//! and the reconciler address commands by value — never by string — so the
//! application never sees a vendor-specific command id.
//!
//! # Layout of the catalogue
//!
//! The variants are grouped in source order to mirror the command catalogue:
//! discovery, live source, health, settings, objects/AI, PTZ, presets,
//! snapshots, zones + zone motion, reconnection, config, queue, composite,
//! and finally the two monitor probes.  `CommandId::order` gives the reconciler
//! a deterministic sweep order (coarse to fine); `CommandId::supports` is the
//! capability gate — a command a camera cannot perform is a typed error, never
//! `unimplemented!()`.

/// A command the device understands.
///
/// Declaration order matches [`CommandId::order`] so the reconciler's sweep is
/// readable from top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandId {
    // -- discovery ---------------------------------------------------------
    /// Enumerate cameras on the network (mDNS, LAN broadcast, USB).
    Discover,
    /// Probe a known IP range for cameras.
    DiscoverByRange,
    /// USB-attached camera (host device).
    ListUsbCameras,
    // -- live source -------------------------------------------------------
    /// Live-source list (network camera / RTSP / IP camera as a "live source").
    LiveSourceList,
    /// Delete a live source registration.
    LiveSourceDelete,
    /// Save a live source (name + URL + auth).
    LiveSourceSave,
    /// Get a saved live source.
    LiveSourceGet,
    // -- health ------------------------------------------------------------
    /// Test connectivity to a camera / live source.
    TestCameraConnection,
    // -- stream ------------------------------------------------------------
    /// Grab one decoded frame.
    GetCameraFrame,
    /// Start a stream at the configured quality.
    StartCameraStream,
    /// Set the global frame rate for all cameras.
    SetGlobalFrameRate,
    /// Set the quality profile for a stream.
    SetQualityProfile,
    // -- health ------------------------------------------------------------
    /// One-shot health probe for a camera.
    MonitorCameraHealth,
    /// Scan for new cameras on the subnet.
    ScanForNewCameras,
    // -- settings ----------------------------------------------------------
    /// Get camera settings (exposure, focus, IR, ...).
    GetCameraSettings,
    /// Update camera settings.
    UpdateCameraSettings,
    // -- objects -----------------------------------------------------------
    /// Detect objects / motion in the current frame.
    DetectObjects,
    // -- PTZ ---------------------------------------------------------------
    /// PTZ move (pan / tilt / zoom / stop).
    PtzMove,
    /// Update camera metadata (friendly name, position, overlay, ...).
    CameraUpdate,
    /// Enable / disable a camera in the dashboard.
    CameraSetEnabled,
    /// Mark a camera as critical (warranty, contract, priority failover).
    CameraSetCritical,
    /// Reorder cameras in the fleet list.
    CameraReorder,
    // -- stats -------------------------------------------------------------
    /// Live stats for one camera.
    CameraGetStats,
    /// Live stats for the whole fleet.
    CameraGetAllStats,
    /// Self-test every camera.
    CameraTestAll,
    // -- presets -----------------------------------------------------------
    /// Save a preset (position + config).
    CameraSavePreset,
    /// Apply a saved preset.
    CameraGotoPreset,
    /// Delete a saved preset.
    CameraDeletePreset,
    // -- snapshots ---------------------------------------------------------
    /// Capture a still image.
    CameraScreenshot,
    // -- settings ----------------------------------------------------------
    /// Reset a camera to its factory settings.
    CameraResetSettings,
    // -- zones -------------------------------------------------------------
    /// List zones.
    ZoneList,
    /// Save a zone.
    ZoneSave,
    /// Delete a zone.
    ZoneDelete,
    /// Run detection over the configured zones.
    RunZoneDetection,
    // -- zone motion -------------------------------------------------------
    /// Get the zone-motion monitor config.
    ZoneMotionGetConfig,
    /// Save the zone-motion monitor config.
    ZoneMotionSaveConfig,
    /// Start the zone-motion monitor.
    ZoneMotionStartMonitor,
    /// Stop the zone-motion monitor.
    ZoneMotionStopMonitor,
    /// List recorded zone-motion events.
    ZoneMotionListEvents,
    /// Acknowledge a zone-motion event.
    ZoneMotionAcknowledgeEvent,
    // -- snapshots ---------------------------------------------------------
    /// Save a snapshot.
    CameraSaveSnapshot,
    /// Delete a snapshot.
    CameraDeleteSnapshot,
    /// List snapshots.
    CameraGetSnapshots,
    /// Does a snapshot exist?
    CameraSnapshotExists,
    // -- reconnection ------------------------------------------------------
    /// Reconnection info for a dropped camera.
    CameraGetReconnectionInfo,
    // -- config ------------------------------------------------------------
    /// Camera config (get / set).
    CameraConfigGet,
    /// Write the camera config.
    CameraConfigSet,
    // -- queue -------------------------------------------------------------
    /// Corrected frame from the queue.
    CameraPopCorrectedImage,
    // -- composite ---------------------------------------------------------
    /// Composite capture (multi-camera).
    CameraStartComposite,
    /// Stop a running composite capture.
    CameraStopComposite,
    // -- AI models ---------------------------------------------------------
    /// AI models (available / set).
    CameraGetAvailableModels,
    /// Select the detection model a camera runs.
    CameraSetDetectionModel,
    /// Turn AI detection on or off for a camera.
    CameraAiDetectionSetEnabled,
    /// Read whether AI detection is on for a camera.
    CameraAiDetectionGetEnabled,
    // -- monitors ----------------------------------------------------------
    /// Health of the camera monitor.
    CameraGetMonitorHealth,
    /// Health of the stream.
    CameraGetMonitorStreamHealth,
}

impl CommandId {
    /// Stable ordering used by the reconciler to sweep the catalogue in a
    /// deterministic pass (coarse to fine).
    pub fn order(self) -> u16 {
        match self {
            Self::Discover => 0,
            Self::DiscoverByRange => 1,
            Self::ListUsbCameras => 2,
            Self::LiveSourceList => 3,
            Self::LiveSourceDelete => 4,
            Self::LiveSourceSave => 5,
            Self::LiveSourceGet => 6,
            Self::TestCameraConnection => 7,
            Self::GetCameraFrame => 8,
            Self::StartCameraStream => 9,
            Self::SetGlobalFrameRate => 10,
            Self::SetQualityProfile => 11,
            Self::MonitorCameraHealth => 12,
            Self::ScanForNewCameras => 13,
            Self::GetCameraSettings => 14,
            Self::UpdateCameraSettings => 15,
            Self::DetectObjects => 16,
            Self::PtzMove => 17,
            Self::CameraUpdate => 18,
            Self::CameraSetEnabled => 19,
            Self::CameraSetCritical => 20,
            Self::CameraReorder => 21,
            Self::CameraGetStats => 22,
            Self::CameraGetAllStats => 23,
            Self::CameraTestAll => 24,
            Self::CameraSavePreset => 25,
            Self::CameraGotoPreset => 26,
            Self::CameraDeletePreset => 27,
            Self::CameraScreenshot => 28,
            Self::CameraResetSettings => 29,
            Self::ZoneList => 30,
            Self::ZoneSave => 31,
            Self::ZoneDelete => 32,
            Self::RunZoneDetection => 33,
            Self::ZoneMotionGetConfig => 34,
            Self::ZoneMotionSaveConfig => 35,
            Self::ZoneMotionStartMonitor => 36,
            Self::ZoneMotionStopMonitor => 37,
            Self::ZoneMotionListEvents => 38,
            Self::ZoneMotionAcknowledgeEvent => 39,
            Self::CameraSaveSnapshot => 40,
            Self::CameraDeleteSnapshot => 41,
            Self::CameraGetSnapshots => 42,
            Self::CameraSnapshotExists => 43,
            Self::CameraGetReconnectionInfo => 44,
            Self::CameraConfigGet => 45,
            Self::CameraConfigSet => 46,
            Self::CameraPopCorrectedImage => 47,
            Self::CameraStartComposite => 48,
            Self::CameraStopComposite => 49,
            Self::CameraGetAvailableModels => 50,
            Self::CameraSetDetectionModel => 51,
            Self::CameraAiDetectionSetEnabled => 52,
            Self::CameraAiDetectionGetEnabled => 53,
            Self::CameraGetMonitorHealth => 54,
            Self::CameraGetMonitorStreamHealth => 55,
        }
    }

    /// Whether a camera of this type is *capable* of this command.  A command a
    /// camera cannot perform is a typed `CameraError`, never an
    /// `unimplemented!()` — capability tables live in the driver.
    pub fn supports(self, ty: crate::CameraType) -> bool {
        match self {
            Self::Discover | Self::DiscoverByRange | Self::ListUsbCameras => true,
            Self::LiveSourceList
            | Self::LiveSourceDelete
            | Self::LiveSourceSave
            | Self::LiveSourceGet => true,
            Self::TestCameraConnection | Self::MonitorCameraHealth | Self::ScanForNewCameras => {
                true
            }
            Self::GetCameraFrame
            | Self::StartCameraStream
            | Self::SetGlobalFrameRate
            | Self::SetQualityProfile => true,
            Self::CameraGetStats | Self::CameraGetAllStats | Self::CameraTestAll => true,
            Self::GetCameraSettings | Self::UpdateCameraSettings | Self::CameraResetSettings => {
                true
            }
            Self::ZoneList | Self::ZoneSave | Self::ZoneDelete | Self::RunZoneDetection => true,
            Self::ZoneMotionGetConfig
            | Self::ZoneMotionSaveConfig
            | Self::ZoneMotionStartMonitor
            | Self::ZoneMotionStopMonitor
            | Self::ZoneMotionListEvents
            | Self::ZoneMotionAcknowledgeEvent => true,
            Self::DetectObjects => matches!(
                ty,
                crate::CameraType::Consumer
                    | crate::CameraType::Business
                    | crate::CameraType::Ptz
                    | crate::CameraType::Indoor
                    | crate::CameraType::Outdoor
                    | crate::CameraType::Ai
            ),
            Self::PtzMove
            | Self::CameraUpdate
            | Self::CameraSetEnabled
            | Self::CameraSetCritical
            | Self::CameraReorder => true,
            Self::CameraSavePreset | Self::CameraGotoPreset | Self::CameraDeletePreset => true,
            Self::CameraScreenshot
            | Self::CameraSaveSnapshot
            | Self::CameraDeleteSnapshot
            | Self::CameraGetSnapshots
            | Self::CameraSnapshotExists => true,
            Self::CameraGetReconnectionInfo | Self::CameraConfigGet | Self::CameraConfigSet => true,
            Self::CameraPopCorrectedImage => true,
            Self::CameraStartComposite | Self::CameraStopComposite => true,
            Self::CameraGetAvailableModels | Self::CameraSetDetectionModel => true,
            Self::CameraAiDetectionSetEnabled | Self::CameraAiDetectionGetEnabled => true,
            Self::CameraGetMonitorHealth | Self::CameraGetMonitorStreamHealth => true,
        }
    }
}

/// A single camera command invocation result.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandResult {
    /// The camera this command targeted (only set for multi-camera commands).
    pub camera_id: Option<String>,
    /// The command id that was run.
    pub command: CommandId,
    /// The result payload, typed per command.
    pub result: CommandResultPayload,
}

impl CommandResult {
    /// Build a result for a command with a concrete payload.
    pub fn new(command: CommandId, result: CommandResultPayload) -> Self {
        Self {
            command,
            result,
            camera_id: None,
        }
    }

    /// Build a result for a command holding a camera id.
    pub fn for_camera(command: CommandId, camera_id: String, result: CommandResultPayload) -> Self {
        Self {
            command,
            result,
            camera_id: Some(camera_id),
        }
    }
}

/// The payload of a command result, summed over the command.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandResultPayload {
    /// One decoded frame (dimensions + bytes).
    Frame {
        /// Frame width in pixels.
        width: u32,
        /// Frame height in pixels.
        height: u32,
        /// Length of the decoded frame buffer in bytes.
        bytes: usize,
    },
    /// A started stream and its handle.
    Stream {
        /// Numeric id of the started stream.
        handle: u64,
        /// Opaque handle for addressing the started stream.
        stream: StreamHandle,
    },
    /// Live stats for a single camera.
    Stats {
        /// Frames per second currently measured.
        fps: f32,
        /// Bytes per second currently received.
        bytes_per_sec: u64,
        /// Whether the camera is currently reachable.
        online: bool,
    },
    /// All statistics for a camera.
    AllStats {
        /// Per-camera snapshots carried in the response.
        cameras: Vec<CameraSnapshot>,
    },
    /// Self-test response.
    Test {
        /// Whether the self-test passed.
        passed: bool,
        /// Human-readable detail for the result.
        detail: String,
    },
    /// Saved presets.
    Presets {
        /// The presets the command returned.
        presets: Vec<Preset>,
    },
    /// Camera settings as JSON.
    Settings {
        /// Raw settings document reported by the device.
        settings: serde_json::Value,
    },
    /// Camera config as JSON.
    Config {
        /// Raw config document reported by the device.
        config: serde_json::Value,
    },
    /// A snapshot reference + dimensions.
    Snapshot {
        /// Path to the stored snapshot.
        path: String,
        /// Snapshot width in pixels.
        width: u32,
        /// Snapshot height in pixels.
        height: u32,
    },
    /// One or more snapshot references.
    Snapshots {
        /// The snapshot references returned by the command.
        snapshots: Vec<SnapshotRef>,
    },
    /// Motion events in a zone.
    MotionEvents {
        /// The motion events returned by the command.
        events: Vec<MotionEvent>,
    },
    /// Reconnection / reconfiguration info.
    Reconfiguration {
        /// Free-form reconnection guidance for the camera.
        info: String,
    },
    /// Composite capture started or not.
    Composite {
        /// Whether the composite capture is now running.
        started: bool,
    },
    /// Available AI models.
    AvailableModels {
        /// Names of the models the camera can run.
        models: Vec<String>,
    },
    /// AI detection status.
    AiStatus {
        /// Whether AI detection is currently enabled.
        enabled: bool,
        /// The selected model name, if any.
        model: Option<String>,
    },
    /// A health probe.
    Health {
        /// Whether the probe judged the camera healthy.
        healthy: bool,
        /// Human-readable detail for the probe.
        detail: String,
    },
    /// No content.
    Empty,
}

impl CommandResultPayload {
    /// Produce the payload a command starts from.  The reconciler and the
    /// facade use this as the "wire" — each `CommandId` maps to one result
    /// shape so the app never has to guess the concrete variant.
    pub fn default_for(command: crate::CommandId) -> Self {
        match command {
            crate::CommandId::GetCameraFrame | crate::CommandId::CameraPopCorrectedImage => {
                CommandResultPayload::Frame {
                    width: 0,
                    height: 0,
                    bytes: 0,
                }
            }
            crate::CommandId::StartCameraStream => CommandResultPayload::Stream {
                handle: 0,
                stream: StreamHandle::new(0),
            },
            crate::CommandId::CameraGetStats => CommandResultPayload::Stats {
                fps: 0.0,
                bytes_per_sec: 0,
                online: false,
            },
            crate::CommandId::CameraGetAllStats => CommandResultPayload::AllStats {
                cameras: Vec::new(),
            },
            crate::CommandId::CameraTestAll => CommandResultPayload::Test {
                passed: false,
                detail: String::new(),
            },
            crate::CommandId::CameraSavePreset
            | crate::CommandId::CameraGotoPreset
            | crate::CommandId::CameraDeletePreset => CommandResultPayload::Presets {
                presets: Vec::new(),
            },
            crate::CommandId::GetCameraSettings
            | crate::CommandId::UpdateCameraSettings
            | crate::CommandId::CameraUpdate
            | crate::CommandId::CameraResetSettings
            | crate::CommandId::CameraConfigGet
            | crate::CommandId::CameraConfigSet => CommandResultPayload::Config {
                config: serde_json::Value::default(),
            },
            crate::CommandId::CameraScreenshot | crate::CommandId::CameraSaveSnapshot => {
                CommandResultPayload::Snapshot {
                    path: String::new(),
                    width: 0,
                    height: 0,
                }
            }
            crate::CommandId::CameraGetSnapshots
            | crate::CommandId::CameraDeleteSnapshot
            | crate::CommandId::CameraSnapshotExists => CommandResultPayload::Snapshots {
                snapshots: Vec::new(),
            },
            crate::CommandId::DetectObjects
            | crate::CommandId::PtzMove
            | crate::CommandId::SetGlobalFrameRate
            | crate::CommandId::SetQualityProfile => CommandResultPayload::Empty,
            crate::CommandId::ZoneList
            | crate::CommandId::ZoneSave
            | crate::CommandId::ZoneDelete
            | crate::CommandId::RunZoneDetection => CommandResultPayload::Empty,
            crate::CommandId::ZoneMotionGetConfig
            | crate::CommandId::ZoneMotionSaveConfig
            | crate::CommandId::ZoneMotionStartMonitor
            | crate::CommandId::ZoneMotionStopMonitor
            | crate::CommandId::ZoneMotionListEvents
            | crate::CommandId::ZoneMotionAcknowledgeEvent => {
                CommandResultPayload::MotionEvents { events: Vec::new() }
            }
            crate::CommandId::TestCameraConnection
            | crate::CommandId::MonitorCameraHealth
            | crate::CommandId::CameraGetMonitorHealth
            | crate::CommandId::CameraGetMonitorStreamHealth => CommandResultPayload::Health {
                healthy: false,
                detail: String::new(),
            },
            crate::CommandId::CameraStartComposite | crate::CommandId::CameraStopComposite => {
                CommandResultPayload::Composite { started: false }
            }
            crate::CommandId::CameraReorder
            | crate::CommandId::CameraSetEnabled
            | crate::CommandId::CameraSetCritical => CommandResultPayload::Empty,
            crate::CommandId::CameraGetReconnectionInfo => CommandResultPayload::Reconfiguration {
                info: String::new(),
            },
            crate::CommandId::CameraGetAvailableModels
            | crate::CommandId::CameraSetDetectionModel => {
                CommandResultPayload::AvailableModels { models: Vec::new() }
            }
            crate::CommandId::CameraAiDetectionSetEnabled
            | crate::CommandId::CameraAiDetectionGetEnabled => CommandResultPayload::AiStatus {
                enabled: false,
                model: None,
            },
            crate::CommandId::Discover
            | crate::CommandId::DiscoverByRange
            | crate::CommandId::ListUsbCameras
            | crate::CommandId::LiveSourceList
            | crate::CommandId::LiveSourceDelete
            | crate::CommandId::LiveSourceSave
            | crate::CommandId::LiveSourceGet
            | crate::CommandId::ScanForNewCameras => CommandResultPayload::Empty,
        }
    }
}

/// A lightweight view of a camera (sent to the frontend).
#[derive(Debug, Clone, PartialEq)]
pub struct CameraSnapshot {
    /// Stable camera identity.
    pub id: String,
    /// Vendor name reported for the camera.
    pub vendor: String,
    /// The camera's type.
    pub ty: crate::CameraType,
    /// Whether the camera is believed to be reachable.
    pub online: bool,
    /// The current belief about what the camera is doing.
    pub state: crate::CameraStateCurrent,
    /// Confidence 0.0..1.0 in `state`.
    pub confidence: f32,
}

/// A saved motion event.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionEvent {
    /// Camera that produced the event.
    pub camera_id: String,
    /// Zone in which motion was detected.
    pub zone_id: String,
    /// Event timestamp in seconds since epoch.
    pub ts: u64,
    /// Detection confidence 0.0..1.0.
    pub confidence: f32,
}

/// A saved preset.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    /// Stable preset identity.
    pub id: String,
    /// Operator-facing preset name.
    pub name: String,
    /// Pan angle in degrees.
    pub pan: f32,
    /// Tilt angle in degrees.
    pub tilt: f32,
    /// Zoom factor.
    pub zoom: f32,
    /// Preset-specific config as JSON.
    pub config: serde_json::Value,
}

/// A saved snapshot reference.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotRef {
    /// Stable snapshot identity.
    pub id: String,
    /// Camera that captured the snapshot.
    pub camera_id: String,
    /// Capture timestamp in seconds since epoch.
    pub created_at: u64,
    /// Snapshot width in pixels.
    pub width: u32,
    /// Snapshot height in pixels.
    pub height: u32,
}

/// An opaque handle to one camera in the fleet.
///
/// Callers receive a `CameraHandle` from discovery or the facade and use it to
/// address a camera without knowing its vendor or transport.  The identity is
/// private so callers cannot forge a handle.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CameraHandle(String);

impl CameraHandle {
    /// Wrap a stable camera identity (MAC, serial, IP or vendor id).
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The stable identity this handle addresses.
    pub fn id(&self) -> &str {
        &self.0
    }
}

/// The handle handed to the caller when a stream is started.
///
/// Opaque: the underlying id is private so callers cannot forge a handle; they
/// only ever get one from `Stream` / `StreamHandle::new`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamHandle(u64);

impl StreamHandle {
    /// Wrap a numeric stream id into an opaque handle.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// The numeric stream id behind this handle.
    pub fn id(self) -> u64 {
        self.0
    }
}
