//! Command inventory for a camera device.
//!
//! `CommandId` is the *what*; the driver crates answer the *how*. The facade and
//! the reconciler talk this enum, so the app never sees a vendor-specific command
//! id.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandId {
    /// Enumerate cameras on the network (mDNS, LAN broadcast, USB).
    Discover,
    /// Probe a known IP range for cameras.
    DiscoverByRange,
    /// USB-attached camera (host device).
    ListUsbCameras,
    /// Live-source list (network camera / RTSP / IP camera as a "live source").
    LiveSourceList,
    /// Delete a live source registration.
    LiveSourceDelete,
    /// Save a live source (name + URL + auth).
    LiveSourceSave,
    /// Get a saved live source.
    LiveSourceGet,
    /// Test connectivity to a camera / live source.
    TestCameraConnection,
    /// Decode one frame (policy: cached or cheap decode only).
    GetCameraFrame,
    /// Start/stop a live stream. Returns a handle to the stream surface.
    StartCameraStream,
    /// Global frame-rate cap for the whole dashboard.
    SetGlobalFrameRate,
    /// Quality profile (bitrate / resolution / fps) for a stream.
    SetQualityProfile,
    /// One-shot health probe for a camera.
    MonitorCameraHealth,
    /// Scan for new cameras on the subnet.
    ScanForNewCameras,
    /// Get camera settings (exposure, focus, IR, etc.).
    GetCameraSettings,
    /// Update camera settings.
    UpdateCameraSettings,
    /// Detect objects / motion in the current frame.
    DetectObjects,
    /// PTZ move (pan / tilt / zoom / stop).
    PtzMove,
    /// Update camera metadata (friendly name, position, overlay, etc.).
    CameraUpdate,
    /// Enable / disable a camera in the dashboard.
    CameraSetEnabled,
    /// Mark a camera as critical (warranty, contract, priority failover).
    CameraSetCritical,
    /// Reorder cameras in the fleet list.
    CameraReorder,
    /// Snapshot statistics for a camera.
    CameraGetStats,
    /// All statistics for a camera.
    CameraGetAllStats,
    /// Self-test a camera.
    CameraTestAll,
    /// Save a preset (position + config).
    CameraSavePreset,
    /// Apply a saved preset.
    CameraGotoPreset,
    /// Delete a saved preset.
    CameraDeletePreset,
    /// Capture a still image.
    CameraScreenshot,
    /// Reset camera to factory defaults.
    CameraResetSettings,
    /// Zone motion detection: list zones.
    ZoneList,
    /// Create / update a zone.
    ZoneSave,
    /// Delete a zone.
    ZoneDelete,
    /// Run zone detection now.
    RunZoneDetection,
    /// Zone motion config (get / save).
    ZoneMotionGetConfig,
    ZoneMotionSaveConfig,
    ZoneMotionStartMonitor,
    ZoneMotionStopMonitor,
    ZoneMotionListEvents,
    ZoneMotionAcknowledgeEvent,
    /// Snapshots (save / delete / list / exists).
    CameraSaveSnapshot,
    CameraDeleteSnapshot,
    CameraGetSnapshots,
    CameraSnapshotExists,
    /// Presets (save / goto / delete).
    CameraSavePreset,
    CameraGotoPreset,
    CameraDeletePreset,
    /// Reconnection info for a dropped camera.
    CameraGetReconnectionInfo,
    /// Camera config (get / set).
    CameraConfigGet,
    CameraConfigSet,
    /// Corrected frame from the queue.
    CameraPopCorrectedImage,
    /// Composite capture (multi-camera).
    CameraStartComposite,
    CameraStopComposite,
    /// AI models (available / set).
    CameraGetAvailableModels,
    CameraSetDetectionModel,
    /// AI detection enable / check.
    CameraAiDetectionSetEnabled,
    CameraAiDetectionGetEnabled,
    /// Health of the camera monitor.
    CameraGetMonitorHealth,
    /// Health of the stream.
    CameraGetMonitorStreamHealth,
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

/// The payload of a command result, summed over the command.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandResultPayload {
    Frame { width: u32, height: u32, bytes: usize },
    Stream { handle: u64, stream: StreamHandle },
    Stats { fps: f32, bytes_per_sec: u64, online: bool },
    AllStats { cameras: Vec<CameraSnapshot> },
    Test { passed: bool, detail: String },
    Presets { presets: Vec<Preset> },
    Settings { settings: serde_json::Value },
    Config { config: serde_json::Value },
    Snapshot { path: String, width: u32, height: u32 },
    Snapshots { snapshots: Vec<SnapshotRef> },
    MotionEvents { events: Vec<MotionEvent> },
    Reconfiguration { info: String },
    Composite { started: bool },
    AvailableModels { models: Vec<String> },
    AiStatus { enabled: bool, model: Option<String> },
    Health { healthy: bool, detail: String },
    Empty,
}

/// A lightweight view of a camera (sent to the frontend).
#[derive(Debug, Clone, PartialEq)]
pub struct CameraSnapshot {
    pub id: String,
    pub vendor: String,
    pub ty: crate::CameraType,
    pub online: bool,
    pub state: crate::CameraStateCurrent,
    pub confidence: f32,
}

/// A saved motion event.
#[derive(Debug, Clone, PartialEq)]
pub struct MotionEvent {
    pub camera_id: String,
    pub zone_id: String,
    pub ts: u64,
    pub confidence: f32,
}

/// A saved preset.
#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub pan: f32,
    pub tilt: f32,
    pub zoom: f32,
    pub config: serde_json::Value,
}

/// A saved snapshot reference.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotRef {
    pub id: String,
    pub camera_id: String,
    pub created_at: u64,
    pub width: u32,
    pub height: u32,
}

/// The handle handed to the caller when a stream is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamHandle(u64);

impl StreamHandle {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}
