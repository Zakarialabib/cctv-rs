//! Camera type taxonomy.
//!
//! A camera is a camera at the core level. The enumeration below is *what the
//! device is*, not how it speaks — that is the driver's job. PTZ, Indoor and
//! Outdoor also carry signal / power quirks; AI carries the detection payload.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CameraType {
    /// Consumer IP camera. Wifi / PoE. Minimal PTZ, typical 2.8mm–4mm lens.
    Consumer,
    /// Business / retail / lobby. 3–8mm lens, timeshift, configurable overlay.
    Business,
    /// Purpose-built PTZ. Heavy pan / tilt / zoom, accessible remote or app.
    Ptz,
    /// Fixed indoor camera.
    Indoor,
    /// Outdoor camera (IP66 / IK10). Needs signal-breach, night / vandal logic.
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
