//! Driver surface for any vendor adapter that plugs into the `cctv-driver`
//! registry.

use cctv_core::{Preset, PtzCommand, StreamInfo, StreamQuality};
/// A platform vendor adapter (Hikvision, Dahua, a legacy bridge, …) that
/// implements the same `cctv_core::CameraBackend` capability surface used by
/// the facade. Nothing here is vendor-specific: any vendor that can speak the
/// shared camera model plugs in here and the facade treats it like any other
/// driver.
pub trait DriverAdapter: Send + Sync {
    /// Stable registry key, e.g. `"hikvision"` / `"ezviz"` / `"generic-rtp"`.
    fn driver_key(&self) -> &'static str;

    /// Human-readable name of the vendor/driver.
    fn name(&self) -> &'static str;

    /// Capabilities this adapter advertises (stream, ptz, presets, …).
    fn capabilities(&self) -> cctv_core::CapabilitySet;

    /// Build a probe report from the adapter's own configuration.
    ///
    /// No network I/O: this is pure derivation from the adapter's config so
    /// that discovery/failure modes stay deterministic.
    fn probe(
        &self,
        id: &str,
    ) -> impl std::future::Future<Output = Result<cctv_core::ProbeResult, cctv_core::CameraError>> + Send;

    /// Stream URL for the requested quality.
    fn stream(
        &self,
        id: &str,
        quality: StreamQuality,
    ) -> impl std::future::Future<Output = Result<StreamInfo, cctv_core::CameraError>> + Send;

    /// PTZ command (pan/tilt/zoom + presets). Not every adapter implements
    /// every subcommand; return `NotImplemented` for the rest.
    fn ptz(
        &self,
        id: &str,
        command: PtzCommand,
    ) -> impl std::future::Future<Output = Result<(), cctv_core::CameraError>> + Send;

    /// Preset list.
    fn presets(
        &self,
        id: &str,
    ) -> impl std::future::Future<Output = Result<Vec<Preset>, cctv_core::CameraError>> + Send;

    /// Save a preset (config is adapter-specific).
    fn save_preset(
        &self,
        id: &str,
        config: &str,
    ) -> impl std::future::Future<Output = Result<(), cctv_core::CameraError>> + Send;

    /// Delete a preset.
    fn delete_preset(
        &self,
        id: &str,
    ) -> impl std::future::Future<Output = Result<(), cctv_core::CameraError>> + Send;

    /// Goto a preset by name.
    fn goto_preset(
        &self,
        id: &str,
        preset: &str,
    ) -> impl std::future::Future<Output = Result<(), cctv_core::CameraError>> + Send;
}
