//! Capability matrix vocabulary and sets.
//!
//! Provides typed capability tokens with dotted identifiers (`ptz.move`, `stream.h264`, etc.),
//! set operations for computing intersections across backend and peer announcements,
//! and UI-facing capability views.

use crate::prelude::*;
use core::fmt;
use core::str::FromStr;

#[cfg(feature = "serde-sync")]
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Well-known dotted capability identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    /// PTZ movement operations (`ptz.move`).
    PtzMove,
    /// PTZ preset operations (`ptz.presets`).
    PtzPresets,
    /// H.264 video streaming (`stream.h264`).
    StreamH264,
    /// H.265 / HEVC video streaming (`stream.h265`).
    StreamH265,
    /// AV1 video streaming (`stream.av1`).
    StreamAv1,
    /// Audio playback / speaker output (`audio.out`).
    AudioOut,
    /// Audio capture / microphone input (`audio.in`).
    AudioIn,
    /// Motion detection alerts and metadata (`detect.motion`).
    DetectMotion,
    /// Flashlight / torch illumination toggle (`light.torch`).
    LightTorch,
    /// Continuous video stream feed (`camera.continuous`).
    CameraContinuous,
    /// 4K resolution (3840x2160 or higher) support (`camera.4k`).
    Camera4k,
}

impl Capability {
    /// String identifier in standard dotted notation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PtzMove => "ptz.move",
            Self::PtzPresets => "ptz.presets",
            Self::StreamH264 => "stream.h264",
            Self::StreamH265 => "stream.h265",
            Self::StreamAv1 => "stream.av1",
            Self::AudioOut => "audio.out",
            Self::AudioIn => "audio.in",
            Self::DetectMotion => "detect.motion",
            Self::LightTorch => "light.torch",
            Self::CameraContinuous => "camera.continuous",
            Self::Camera4k => "camera.4k",
        }
    }

    /// All known capabilities.
    pub const ALL: &'static [Capability] = &[
        Capability::PtzMove,
        Capability::PtzPresets,
        Capability::StreamH264,
        Capability::StreamH265,
        Capability::StreamAv1,
        Capability::AudioOut,
        Capability::AudioIn,
        Capability::DetectMotion,
        Capability::LightTorch,
        Capability::CameraContinuous,
        Capability::Camera4k,
    ];
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when parsing an unrecognized capability string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseCapabilityError(pub String);

impl fmt::Display for ParseCapabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown capability: {:?}", self.0)
    }
}

impl core::error::Error for ParseCapabilityError {}

impl FromStr for Capability {
    type Err = ParseCapabilityError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "ptz.move" => Ok(Self::PtzMove),
            "ptz.presets" => Ok(Self::PtzPresets),
            "stream.h264" => Ok(Self::StreamH264),
            "stream.h265" => Ok(Self::StreamH265),
            "stream.av1" => Ok(Self::StreamAv1),
            "audio.out" => Ok(Self::AudioOut),
            "audio.in" => Ok(Self::AudioIn),
            "detect.motion" => Ok(Self::DetectMotion),
            "light.torch" => Ok(Self::LightTorch),
            "camera.continuous" => Ok(Self::CameraContinuous),
            "camera.4k" => Ok(Self::Camera4k),
            other => Err(ParseCapabilityError(other.to_string())),
        }
    }
}

#[cfg(feature = "serde-sync")]
impl Serialize for Capability {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

#[cfg(feature = "serde-sync")]
impl<'de> Deserialize<'de> for Capability {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// A compact, ordered set of capabilities.
///
/// Implemented using an internal bitmask for zero-allocation efficiency while maintaining
/// pure `alloc`-only compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct CapabilitySet(u32);

impl CapabilitySet {
    /// An empty capability set.
    pub const EMPTY: Self = Self(0);

    /// A capability set containing all known capabilities.
    pub const ALL: Self = Self((1 << Capability::ALL.len()) - 1);

    const fn bit(cap: Capability) -> u32 {
        1 << (cap as u32)
    }

    /// Create an empty set.
    pub const fn new() -> Self {
        Self(0)
    }

    /// Add a capability to the set.
    pub fn insert(&mut self, cap: Capability) -> &mut Self {
        self.0 |= Self::bit(cap);
        self
    }

    /// Remove a capability from the set.
    pub fn remove(&mut self, cap: Capability) -> &mut Self {
        self.0 &= !Self::bit(cap);
        self
    }

    /// Check whether a capability is in the set.
    pub const fn contains(&self, cap: Capability) -> bool {
        (self.0 & Self::bit(cap)) != 0
    }

    /// Check whether this set is completely empty.
    pub const fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// Number of capabilities present in the set.
    pub const fn len(&self) -> usize {
        self.0.count_ones() as usize
    }

    /// Compute the intersection with another capability set.
    pub const fn intersection(&self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Compute the union with another capability set.
    pub const fn union(&self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Compute the difference with another capability set (`self \ other`).
    pub const fn difference(&self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Check if this set is a subset of `other`.
    pub const fn is_subset(&self, other: &Self) -> bool {
        (self.0 & !other.0) == 0
    }

    /// Check if this set has any capabilities in common with `other`.
    pub const fn intersects(&self, other: &Self) -> bool {
        (self.0 & other.0) != 0
    }

    /// Iterate over the capabilities present in the set.
    pub fn iter(&self) -> impl Iterator<Item = Capability> {
        let mask = self.0;
        Capability::ALL
            .iter()
            .copied()
            .filter(move |&c| (mask & Self::bit(c)) != 0)
    }

    /// Build a CapabilitySet from a slice of capabilities.
    pub fn from_slice(caps: &[Capability]) -> Self {
        let mut set = Self::new();
        for &cap in caps {
            set.insert(cap);
        }
        set
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<T: IntoIterator<Item = Capability>>(iter: T) -> Self {
        let mut set = Self::new();
        for cap in iter {
            set.insert(cap);
        }
        set
    }
}

impl core::ops::BitAnd for CapabilitySet {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self::Output {
        self.intersection(rhs)
    }
}

impl core::ops::BitOr for CapabilitySet {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        self.union(rhs)
    }
}

#[cfg(feature = "serde-sync")]
impl Serialize for CapabilitySet {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        for cap in self.iter() {
            seq.serialize_element(cap.as_str())?;
        }
        seq.end()
    }
}

#[cfg(feature = "serde-sync")]
impl<'de> Deserialize<'de> for CapabilitySet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let list = Vec::<String>::deserialize(deserializer)?;
        let mut set = CapabilitySet::new();
        for item in list {
            let cap: Capability = item.parse().map_err(serde::de::Error::custom)?;
            set.insert(cap);
        }
        Ok(set)
    }
}

/// Computed view of supported capabilities resulting from a peer handshake.
///
/// UI layers consume this view: buttons and controls for actions (e.g. PTZ control,
/// torch toggle, 4K selection) are only rendered if the intersection of camera and peer
/// capabilities contains the required capability set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct CapabilityView {
    /// Capabilities actively supported and negotiated by both sides.
    pub active: CapabilitySet,
    /// Capabilities supported by the device backend.
    pub backend: CapabilitySet,
    /// Capabilities claimed by the peer / client.
    pub peer: CapabilitySet,
}

impl CapabilityView {
    /// Compute a view from backend capabilities and peer capabilities.
    pub const fn compute(backend: CapabilitySet, peer: CapabilitySet) -> Self {
        Self {
            active: backend.intersection(peer),
            backend,
            peer,
        }
    }

    /// Check whether a capability is supported by the negotiated intersection.
    pub const fn is_supported(&self, cap: Capability) -> bool {
        self.active.contains(cap)
    }

    /// Check whether a required capability set is completely supported by the negotiated intersection.
    ///
    /// UI elements check this to determine whether to render buttons or controls.
    pub const fn allows_action(&self, required: CapabilitySet) -> bool {
        required.is_subset(&self.active)
    }

    /// Check if PTZ move controls should be displayed.
    pub const fn can_ptz_move(&self) -> bool {
        self.active.contains(Capability::PtzMove)
    }

    /// Check if PTZ preset buttons should be displayed.
    pub const fn can_ptz_presets(&self) -> bool {
        self.active.contains(Capability::PtzPresets)
    }

    /// Check if torch toggle control should be displayed.
    pub const fn can_torch(&self) -> bool {
        self.active.contains(Capability::LightTorch)
    }

    /// Check if motion detection is active.
    pub const fn can_detect_motion(&self) -> bool {
        self.active.contains(Capability::DetectMotion)
    }
}

/// Declarative registry of standard backend capability sets.
pub struct BackendCapabilityRegistry;

impl BackendCapabilityRegistry {
    /// Declarative capability set for Ezviz cloud driver backend.
    pub fn ezviz() -> CapabilitySet {
        CapabilitySet::from_slice(&[
            Capability::StreamH264,
            Capability::StreamH265,
            Capability::PtzMove,
            Capability::CameraContinuous,
            Capability::DetectMotion,
            Capability::LightTorch,
        ])
    }

    /// Declarative capability set for ONVIF driver backend.
    pub fn onvif() -> CapabilitySet {
        CapabilitySet::from_slice(&[
            Capability::StreamH264,
            Capability::StreamH265,
            Capability::PtzMove,
            Capability::PtzPresets,
            Capability::CameraContinuous,
            Capability::AudioIn,
            Capability::AudioOut,
        ])
    }

    /// Declarative capability set for UVC host capture adapter.
    pub fn uvc() -> CapabilitySet {
        CapabilitySet::from_slice(&[
            Capability::StreamH264,
            Capability::CameraContinuous,
            Capability::AudioIn,
        ])
    }

    /// Declarative capability set for PTP capture adapter.
    pub fn ptp() -> CapabilitySet {
        CapabilitySet::from_slice(&[Capability::Camera4k])
    }

    /// Declarative capability set for RTSP network driver backend.
    pub fn rtsp() -> CapabilitySet {
        CapabilitySet::from_slice(&[
            Capability::StreamH264,
            Capability::StreamH265,
            Capability::CameraContinuous,
            Capability::AudioIn,
        ])
    }

    /// Declarative capability set for mock test camera backend.
    pub fn mock() -> CapabilitySet {
        CapabilitySet::from_slice(&[
            Capability::StreamH264,
            Capability::StreamH265,
            Capability::PtzMove,
            Capability::PtzPresets,
            Capability::CameraContinuous,
        ])
    }

    /// Lookup declarative capability set by backend driver key.
    pub fn for_driver(driver_key: &str) -> Option<CapabilitySet> {
        match driver_key {
            "ezviz" => Some(Self::ezviz()),
            "onvif" => Some(Self::onvif()),
            "uvc" => Some(Self::uvc()),
            "ptp" => Some(Self::ptp()),
            "rtsp" => Some(Self::rtsp()),
            "mock" => Some(Self::mock()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dotted_identifiers() {
        assert_eq!(Capability::PtzMove.as_str(), "ptz.move");
        assert_eq!(Capability::PtzPresets.as_str(), "ptz.presets");
        assert_eq!(Capability::StreamH264.as_str(), "stream.h264");
        assert_eq!(Capability::StreamH265.as_str(), "stream.h265");
        assert_eq!(Capability::StreamAv1.as_str(), "stream.av1");
        assert_eq!(Capability::AudioOut.as_str(), "audio.out");
        assert_eq!(Capability::AudioIn.as_str(), "audio.in");
        assert_eq!(Capability::DetectMotion.as_str(), "detect.motion");
        assert_eq!(Capability::LightTorch.as_str(), "light.torch");
        assert_eq!(Capability::CameraContinuous.as_str(), "camera.continuous");
        assert_eq!(Capability::Camera4k.as_str(), "camera.4k");

        for cap in Capability::ALL {
            assert_eq!(cap.as_str().parse::<Capability>().unwrap(), *cap);
            assert_eq!(cap.to_string(), cap.as_str());
        }
    }

    #[test]
    fn test_set_operations() {
        let mut set1 = CapabilitySet::new();
        set1.insert(Capability::StreamH264)
            .insert(Capability::PtzMove);

        let mut set2 = CapabilitySet::new();
        set2.insert(Capability::StreamH264)
            .insert(Capability::StreamH265);

        let inter = set1.intersection(set2);
        assert_eq!(inter.len(), 1);
        assert!(inter.contains(Capability::StreamH264));
        assert!(!inter.contains(Capability::StreamH265));
        assert!(!inter.contains(Capability::PtzMove));

        let union = set1.union(set2);
        assert_eq!(union.len(), 3);
        assert!(union.contains(Capability::StreamH264));
        assert!(union.contains(Capability::StreamH265));
        assert!(union.contains(Capability::PtzMove));

        assert!(inter.is_subset(&set1));
        assert!(inter.is_subset(&set2));
    }

    #[test]
    fn test_capability_view_button_render_logic() {
        let mut backend = CapabilitySet::new();
        backend
            .insert(Capability::StreamH264)
            .insert(Capability::StreamH265)
            .insert(Capability::PtzMove);

        // Peer only has stream.h264
        let mut peer = CapabilitySet::new();
        peer.insert(Capability::StreamH264);

        let view = CapabilityView::compute(backend, peer);
        assert!(view.is_supported(Capability::StreamH264));
        assert!(!view.is_supported(Capability::StreamH265));
        assert!(!view.is_supported(Capability::PtzMove));
        assert!(!view.can_ptz_move());

        // UI requires stream.h264
        assert!(view.allows_action(CapabilitySet::from_slice(&[Capability::StreamH264])));

        // UI requires PTZ move -> should NOT render button
        assert!(!view.allows_action(CapabilitySet::from_slice(&[Capability::PtzMove])));
    }
}
