//! The facade gate.
//!
//! What SignageMaster's `cctv.ts` talks to.  One [`Facade`] handle, no vendor
//! names: every method speaks `cctv-core` types, and the drivers only settle
//! them.
//!
//! # Return types
//!
//! Two shapes, on purpose:
//!
//! - A command with a device-model equivalent returns it — `get_camera_frame`
//!   gives back a [`Snapshot`] (the image bytes), `start_camera_stream` a
//!   [`StreamInfo`], `camera_get_presets` the presets.
//! - A command whose result has no device-model equivalent returns a
//!   [`CommandResult`], carrying the [`CommandResultPayload`] variant that names
//!   it — health probes, stats, reconnection info, the fleet self-test.
//!
//! [`Facade::execute`] is the catalogue entry point: it routes the
//! parameterless commands by [`CommandId`] and answers `NotImplemented` for the
//! rest, which have typed methods instead.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use cctv_agent::{Audit, Reconciler, Remediation};
use cctv_core::{
    CameraBackend, CameraConfig, CameraDesired, CameraError, CameraHandle, CameraSnapshot,
    CameraState, CameraStateCurrent, CommandId, CommandResult, CommandResultPayload, Preset,
    PtzCommand, Snapshot, StreamInfo, StreamQuality,
};
use cctv_discovery::{Datagram, DiscoveredCamera, Probe};
use cctv_transport::reader::FrameReader;

use crate::drivers::{DriverFactory, EZVIZ, ONVIF};

/// The clock frame statistics are measured on: Unix milliseconds.
///
/// [`FrameReader`] is pure — it never reads a clock — so whoever pushes frames
/// stamps them, and whoever reads the rates supplies "now". Both must use the
/// same clock, and this is it.
pub fn clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// One registered camera: its connection record and the driver that serves it.
struct Registered {
    config: CameraConfig,
    backend: Arc<dyn CameraBackend>,
}

/// The single handle the application holds onto; it fronts the whole camera
/// stack.
///
/// Cameras are registered against a driver — either by handing the facade a
/// backend ([`Facade::register`]) or by letting the driver registry build one
/// from the connection record ([`Facade::attach`]).
pub struct Facade {
    cameras: BTreeMap<String, Registered>,
    drivers: BTreeMap<String, DriverFactory>,
    /// One frame reader per camera, shared with the application's stream
    /// consumer.  The facade measures from it; it never pushes into it, because
    /// the bytes belong to whoever owns the wire.
    readers: BTreeMap<String, Arc<Mutex<FrameReader>>>,
    reconciler: Reconciler,
}

impl Facade {
    /// Create an empty facade for the application to wire with drivers.
    pub fn new() -> Self {
        Self {
            cameras: BTreeMap::new(),
            drivers: BTreeMap::new(),
            readers: BTreeMap::new(),
            reconciler: Reconciler::new(Audit::new()),
        }
    }

    /// Register the driver factories this workspace ships.
    ///
    /// Today that is `ezviz`, built from the camera's connection record (see
    /// [`crate::drivers::ezviz`] for the field mapping).
    pub fn with_builtin_drivers(mut self) -> Self {
        self.register_driver(EZVIZ, Arc::new(crate::drivers::ezviz));
        self.register_driver(ONVIF, Arc::new(crate::drivers::onvif));
        self
    }

    // -- driver registry ---------------------------------------------------

    /// Register a driver factory under a key.
    ///
    /// The key is what [`CameraConfig::driver`] names; re-registering a key
    /// replaces the factory.
    pub fn register_driver(&mut self, key: impl Into<String>, factory: DriverFactory) {
        self.drivers.insert(key.into(), factory);
    }

    /// The keys a factory is registered under, in order.
    pub fn driver_keys(&self) -> Vec<String> {
        self.drivers.keys().cloned().collect()
    }

    /// Build a camera's backend from its connection record and register it.
    ///
    /// A key with no registered factory is a typed error, not a panic.
    pub fn attach(&mut self, config: CameraConfig) -> Result<CameraHandle, CameraError> {
        let factory = self.drivers.get(&config.driver).cloned().ok_or_else(|| {
            CameraError::Other(format!("no driver registered for {:?}", config.driver))
        })?;
        let backend = factory(&config)?;
        Ok(self.register(config, backend))
    }

    // -- camera registry ---------------------------------------------------

    /// Register a camera and the driver that serves it.
    ///
    /// Re-registering an id replaces the previous entry, so a re-discovery is
    /// idempotent.
    pub fn register(
        &mut self,
        config: CameraConfig,
        backend: Arc<dyn CameraBackend>,
    ) -> CameraHandle {
        let id = config.id.clone();
        self.cameras
            .insert(id.clone(), Registered { config, backend });
        // A re-registration keeps the existing reader: the stream may still be
        // open, and throwing the measurement away would report a false zero.
        self.readers
            .entry(id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(FrameReader::new())));
        CameraHandle::new(id)
    }

    /// The frame reader for a camera, shared with the stream consumer.
    ///
    /// The application pushes frames into it as they come off the wire and pulls
    /// the newest usable one on its own tick; the facade reads the measurements
    /// out for [`Facade::camera_get_stats`].  `None` when the camera is not
    /// registered.
    ///
    /// The clock is the application's: stamp frames with [`clock_ms`] so the
    /// windowed rates mean what they say.
    pub fn frame_reader(&self, id: &str) -> Option<Arc<Mutex<FrameReader>>> {
        self.readers.get(id).cloned()
    }

    /// Remove a camera from the fleet, returning its connection record.
    pub fn unregister(&mut self, id: &str) -> Option<CameraConfig> {
        self.readers.remove(id);
        self.cameras.remove(id).map(|registered| registered.config)
    }

    /// The currently known cameras as device-model handles, ordered by id.
    pub fn list_cameras(&self) -> Vec<CameraHandle> {
        self.cameras
            .keys()
            .cloned()
            .map(CameraHandle::new)
            .collect()
    }

    /// Whether a camera is registered.
    pub fn contains(&self, id: &str) -> bool {
        self.cameras.contains_key(id)
    }

    /// The connection record for one camera, credentials included.
    ///
    /// Hand this to nothing that logs; [`Facade::camera_config_get`] is the
    /// redacted form.
    pub fn config(&self, id: &str) -> Option<&CameraConfig> {
        self.cameras.get(id).map(|registered| &registered.config)
    }

    /// The driver serving a camera, or a typed error.
    fn backend(&self, id: &str) -> Result<&Arc<dyn CameraBackend>, CameraError> {
        self.cameras
            .get(id)
            .map(|registered| &registered.backend)
            .ok_or(CameraError::NoCamera)
    }

    // -- discovery ---------------------------------------------------------

    /// Sweep the LAN and report the cameras found, without registering them.
    ///
    /// Registration needs a driver for each camera, which is the application's
    /// choice.  This blocks for the probe's collection window (two multicast
    /// exchanges), so it belongs on a worker thread; [`Facade::discover_with`]
    /// is the injectable form the tests use.
    pub fn discover(&self) -> Vec<DiscoveredCamera> {
        Probe::new().scan()
    }

    /// The same sweep, over an injected transport.
    pub fn discover_with(&self, net: &dyn Datagram) -> Vec<DiscoveredCamera> {
        Probe::new().scan_with(net)
    }

    // -- reconciler --------------------------------------------------------

    /// The reconciler the facade drives.
    pub fn reconciler(&self) -> &Reconciler {
        &self.reconciler
    }

    /// The reconciler, mutably — for `set_desired` and `record_error`.
    pub fn reconciler_mut(&mut self) -> &mut Reconciler {
        &mut self.reconciler
    }

    /// Probe every registered camera and reconcile the fleet against the desired
    /// state the application has set.
    ///
    /// A camera that fails to probe has its error recorded, so the reconciler
    /// grades it from the error taxonomy rather than from a state it cannot
    /// trust.
    pub async fn reconcile(&mut self) -> Vec<Remediation> {
        let mut fleet = Vec::with_capacity(self.cameras.len());
        for (id, registered) in &self.cameras {
            let mut state = CameraState::new(
                id.clone(),
                registered.config.driver.clone(),
                registered.config.ty,
            );
            match registered.backend.probe().await {
                Ok(probe) => {
                    state.vendor = probe.vendor.clone();
                    state.apply_probe(probe, true, CameraStateCurrent::On, 1.0);
                }
                Err(error) => self.reconciler.record_error(id.clone(), error),
            }
            fleet.push(state);
        }
        self.reconciler.reconcile_all(&mut fleet)
    }

    // -- commands: device-model results ------------------------------------

    /// `get_camera_frame`: one still from the camera, with its bytes.
    ///
    /// The command catalogue's `Frame` payload carries only dimensions and a
    /// length, so the image itself travels here.
    pub async fn get_camera_frame(&self, id: &str) -> Result<Snapshot, CameraError> {
        self.backend(id)?.snapshot().await
    }

    /// `start_camera_stream`: resolve the playable URI at this quality.
    pub async fn start_camera_stream(
        &self,
        id: &str,
        quality: StreamQuality,
    ) -> Result<StreamInfo, CameraError> {
        self.backend(id)?.stream(quality).await
    }

    /// `ptz_move`: move the camera, in the device-model vocabulary.
    pub async fn ptz_move(&self, id: &str, command: PtzCommand) -> Result<(), CameraError> {
        self.backend(id)?.ptz(command).await
    }

    /// `camera_get_presets`: the presets stored on the device.
    pub async fn camera_get_presets(&self, id: &str) -> Result<Vec<Preset>, CameraError> {
        self.backend(id)?.presets().await
    }

    /// `camera_save_preset`: store the current position under `name`.
    pub async fn camera_save_preset(&self, id: &str, name: &str) -> Result<Preset, CameraError> {
        self.backend(id)?.save_preset(name, None).await
    }

    /// `camera_goto_preset`: move to a stored preset.
    pub async fn camera_goto_preset(&self, id: &str, preset_id: &str) -> Result<(), CameraError> {
        self.backend(id)?.goto_preset(preset_id).await
    }

    /// `camera_delete_preset`: delete a stored preset.
    pub async fn camera_delete_preset(&self, id: &str, preset_id: &str) -> Result<(), CameraError> {
        self.backend(id)?.delete_preset(preset_id).await
    }

    /// `camera_config_get`: the connection record, password redacted.
    pub fn camera_config_get(&self, id: &str) -> Result<CameraConfig, CameraError> {
        self.config(id)
            .map(CameraConfig::redacted)
            .ok_or(CameraError::NoCamera)
    }

    /// `camera_config_set`: replace a camera's connection record.
    ///
    /// The driver is not rebuilt here — re-register the camera with
    /// [`Facade::attach`] when the connection itself changes.
    pub fn camera_config_set(&mut self, config: CameraConfig) -> Result<(), CameraError> {
        let registered = self
            .cameras
            .get_mut(&config.id)
            .ok_or(CameraError::NoCamera)?;
        registered.config = config;
        Ok(())
    }

    /// `camera_set_enabled`: include or exclude a camera from the fleet.
    ///
    /// This sets the reconciler's intent; the camera follows on the next
    /// [`Facade::reconcile`].
    pub fn camera_set_enabled(&mut self, id: &str, enabled: bool) -> Result<(), CameraError> {
        if !self.contains(id) {
            return Err(CameraError::NoCamera);
        }
        let desired = if enabled {
            CameraDesired::Idle
        } else {
            CameraDesired::Disabled
        };
        self.reconciler.set_desired(id, desired);
        Ok(())
    }

    // -- commands: catalogue results ---------------------------------------

    /// `test_camera_connection`: is the camera reachable right now?
    ///
    /// The probe's failure is reported *in* the result, because the command
    /// succeeded in determining reachability — a device that is down is an
    /// answer, not an error.
    pub async fn test_camera_connection(&self, id: &str) -> Result<CommandResult, CameraError> {
        let registered = self.backend(id)?;
        let (healthy, detail) = match registered.probe().await {
            Ok(probe) => (true, format!("{} {}", probe.vendor, probe.model)),
            Err(error) => (false, error.to_string()),
        };
        Ok(CommandResult::for_camera(
            CommandId::TestCameraConnection,
            id.to_string(),
            CommandResultPayload::Health { healthy, detail },
        ))
    }

    /// `monitor_camera_health`: reachability **and** stream resolution.
    pub async fn monitor_camera_health(&self, id: &str) -> Result<CommandResult, CameraError> {
        let registered = self.backend(id)?;
        let probe = registered.probe().await;
        let stream = registered.stream(StreamQuality::Main).await;
        let (healthy, detail) = match (&probe, &stream) {
            (Ok(probe), Ok(stream)) => (
                true,
                format!(
                    "{} {} — {}",
                    probe.vendor,
                    probe.model,
                    stream.redacted_uri()
                ),
            ),
            (Ok(probe), Err(error)) => (
                false,
                format!("{} {} — stream failed: {error}", probe.vendor, probe.model),
            ),
            (Err(error), _) => (false, format!("probe failed: {error}")),
        };
        Ok(CommandResult::for_camera(
            CommandId::MonitorCameraHealth,
            id.to_string(),
            CommandResultPayload::Health { healthy, detail },
        ))
    }

    /// `camera_get_stats`: live statistics for one camera.
    ///
    /// **`fps` and `bytes_per_sec` are zero today.**  Nothing measures them
    /// until `cctv-transport`'s frame reader is wired into the stream path (see
    /// `docs/roadmap.md`); only `online` is measured, from a probe.
    pub async fn camera_get_stats(&self, id: &str) -> Result<CommandResult, CameraError> {
        let registered = self.backend(id)?;
        let online = registered.probe().await.is_ok();

        // Measured, not assumed: the numbers come from the frames the stream
        // consumer actually handed the reader. Zero means nothing is flowing —
        // which is the truth until something pushes into the reader.
        let now = clock_ms();
        let (fps, bytes_per_sec) = match self.readers.get(id) {
            Some(reader) => match reader.lock() {
                Ok(reader) => (reader.fps(now), reader.bytes_per_sec(now)),
                Err(_) => (0.0, 0),
            },
            None => (0.0, 0),
        };

        Ok(CommandResult::for_camera(
            CommandId::CameraGetStats,
            id.to_string(),
            CommandResultPayload::Stats {
                fps,
                bytes_per_sec,
                online,
            },
        ))
    }

    /// `camera_get_reconnection_info`: whether the camera is reachable, and
    /// through which driver.
    ///
    /// A query reports; it does not fail.  An unreachable camera comes back as
    /// `Ok` with the reason in the payload.
    pub async fn camera_get_reconnection_info(
        &self,
        id: &str,
    ) -> Result<CommandResult, CameraError> {
        let registered = self.backend(id)?;
        let info = match registered.probe().await {
            Ok(probe) => format!(
                "reachable: {} {} via {}",
                probe.vendor,
                probe.model,
                registered.driver()
            ),
            Err(error) => format!("unreachable: {error}"),
        };
        Ok(CommandResult::for_camera(
            CommandId::CameraGetReconnectionInfo,
            id.to_string(),
            CommandResultPayload::Reconfiguration { info },
        ))
    }

    /// `camera_test_all`: probe every registered camera.
    pub async fn camera_test_all(&self) -> Result<CommandResult, CameraError> {
        let mut failures = Vec::new();
        for (id, registered) in &self.cameras {
            if let Err(error) = registered.backend.probe().await {
                failures.push(format!("{id}: {error}"));
            }
        }
        let total = self.cameras.len();
        Ok(CommandResult::new(
            CommandId::CameraTestAll,
            CommandResultPayload::Test {
                passed: failures.is_empty(),
                detail: if failures.is_empty() {
                    format!("{total} camera(s) reachable")
                } else {
                    failures.join("; ")
                },
            },
        ))
    }

    /// `camera_get_all_stats`: one snapshot per registered camera.
    ///
    /// This probes every camera, so it is a fleet-wide call, not a cheap read.
    pub async fn camera_get_all_stats(&self) -> Result<CommandResult, CameraError> {
        let mut cameras = Vec::with_capacity(self.cameras.len());
        for (id, registered) in &self.cameras {
            let probe = registered.backend.probe().await;
            let online = probe.is_ok();
            cameras.push(CameraSnapshot {
                id: id.clone(),
                // An offline camera reports no vendor, rather than a guess.
                vendor: probe.map(|probe| probe.vendor).unwrap_or_default(),
                ty: registered.config.ty,
                online,
                state: if online {
                    CameraStateCurrent::On
                } else {
                    CameraStateCurrent::Off
                },
                confidence: if online { 1.0 } else { 0.0 },
            });
        }
        Ok(CommandResult::new(
            CommandId::CameraGetAllStats,
            CommandResultPayload::AllStats { cameras },
        ))
    }

    /// Run a parameterless catalogue command by id.
    ///
    /// Commands that need arguments have typed methods instead, and come back
    /// here as `NotImplemented` rather than being silently ignored.
    pub async fn execute(
        &self,
        id: &str,
        command: CommandId,
    ) -> Result<CommandResult, CameraError> {
        match command {
            CommandId::TestCameraConnection
            | CommandId::CameraGetMonitorHealth
            | CommandId::CameraGetMonitorStreamHealth => self.test_camera_connection(id).await,
            CommandId::MonitorCameraHealth => self.monitor_camera_health(id).await,
            CommandId::CameraGetStats => self.camera_get_stats(id).await,
            CommandId::CameraGetReconnectionInfo => self.camera_get_reconnection_info(id).await,
            CommandId::CameraConfigGet => {
                let config = self.camera_config_get(id)?;
                Ok(CommandResult::for_camera(
                    command,
                    id.to_string(),
                    CommandResultPayload::Config {
                        config: serde_json::to_value(config)
                            .map_err(|error| CameraError::Other(error.to_string()))?,
                    },
                ))
            }
            CommandId::CameraGetPresets => {
                let presets = self.camera_get_presets(id).await?;
                Ok(CommandResult::for_camera(
                    command,
                    id.to_string(),
                    CommandResultPayload::Presets { presets },
                ))
            }
            _ => Err(CameraError::NotImplemented),
        }
    }
}

impl Default for Facade {
    fn default() -> Self {
        Self::new()
    }
}
