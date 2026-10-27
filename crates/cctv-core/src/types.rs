//! Camera type taxonomy.
//!
//! A camera is a camera at the core level.  The enumeration below is *what the
//! device is*, not *how it speaks* — that is the driver's job.  PTZ, Indoor and
//! Outdoor also carry signal / power quirks; AI carries the detection payload.
//!
//! Rules the reconciler and the facade depend on:
//!
//! - `CameraType` is a value, not a trait.  Drivers **must not** add variants.
//! - PTZ is a *capability* (`ptz_supported` in `ProbeResult`), not part of the
//!   type.  A `Business` camera may pan / tilt.
//! - AI is a *capability*, not a type.  A camera can be `Consumer` and have AI.
//! - Ordering `Consumer → Business → Ptz → Indoor → Outdoor → Ai` is the
//!   stable, committee-ordered coarse-to-specific sequence the fleet
//!   reconciler relies on.

/// The kind of device a camera is, independent of how it is driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CameraType {
    /// Consumer IP camera.  WiFi / PoE.  Minimal PTZ, typical 2.8mm–4mm lens.
    Consumer,
    /// Business / retail / lobby.  3–8mm lens, timeshift, configurable overlay.
    Business,
    /// Purpose-built PTZ.  Heavy pan / tilt / zoom, accessible remote or app.
    Ptz,
    /// Fixed indoor camera.
    Indoor,
    /// Outdoor camera (IP66 / IK10).  Needs signal-breach, night / vandal logic.
    Outdoor,
    /// AI camera with dedicated detection payload and badge reader.
    Ai,
}

impl CameraType {
    /// Stable ordering used by the fleet reconciler (coarse → specific).
    pub fn order(self) -> u8 {
        match self {
            Self::Consumer => 0,
            Self::Business => 1,
            Self::Ptz => 2,
            Self::Indoor => 3,
            Self::Outdoor => 4,
            Self::Ai => 5,
        }
    }

    /// Every camera type, in reconciler order.
    pub fn all() -> &'static [Self] {
        &[
            Self::Consumer,
            Self::Business,
            Self::Ptz,
            Self::Indoor,
            Self::Outdoor,
            Self::Ai,
        ]
    }
}

/// The physical or protocol link layer for reaching a camera device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default)]
pub enum LinkKind {
    /// IP network connection (ONVIF, RTSP, HTTP/TCP/UDP).
    #[default]
    Network,
    /// USB direct host attachment.
    Usb,
    /// Cloud platform or vendor platform relay.
    Cloud,
    /// Unknown or unspecified link layer.
    Unknown,
}

impl LinkKind {
    /// Whether this link is a USB connection.
    pub fn is_usb(self) -> bool {
        matches!(self, Self::Usb)
    }

    /// Whether this link is a network connection.
    pub fn is_network(self) -> bool {
        matches!(self, Self::Network)
    }
}

/// Hardware classification for attached or discovered devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default)]
pub enum DeviceClass {
    /// Video capture / camera device (e.g. UVC).
    Video,
    /// Audio input/output device (e.g. UAC microphone or speaker).
    Audio,
    /// Human Interface Device.
    Hid,
    /// Composite device combining multiple device classes (e.g. video + audio).
    Composite,
    /// Vendor-specific hardware device class.
    VendorSpecific,
    /// Unknown hardware device class.
    #[default]
    Unknown,
}

impl DeviceClass {
    /// Whether this device class includes video capture.
    pub fn is_video(self) -> bool {
        matches!(self, Self::Video | Self::Composite)
    }

    /// Whether this device class includes audio.
    pub fn is_audio(self) -> bool {
        matches!(self, Self::Audio | Self::Composite)
    }
}
