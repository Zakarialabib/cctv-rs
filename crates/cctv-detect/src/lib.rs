//! Detection — the device model for motion / AI object detection.
//!
//! Detection is a **report**, not a command: a camera tells the fleet where it
//! saw motion, and the fleet decides whether to re-derive it from a live frame.
//! The detector crate carries the *what* — the content a camera raised — and the
//! reconciler and the application decide the *what now*.
//!
//! This crate is deliberately thin: it holds the detection result, the zone it
//! fired in, and a confidence score.  It is deliberately free of I/O, the camera
//! driver owns the frame, and the reconciler owns the response.
//!
//! # Layout
//!
//! - [`Detection`] — one detection event (motion blob, object, ...).
//! - [`Zone`] — the boundary a detection fired inside; published, not owned.
//! - [`ZoneConfig`] — the detection monitor settings a user can read and save.
//!
//! # No panics
//!
//! The public surface is total on its inputs: a `ZoneConfig` that asks for a
//! window wider than the frame is clamped, not rejected, and a `None` threshold
//! is a backstop, not a caller error.

#![forbid(unsafe_code)]

pub mod zone;

pub use zone::{Region, RegionKind, Zone, ZoneConfig};

/// One detection fired by a camera in a zone.
///
/// A detection is what the camera observed, not what the fleet is allowed to do
/// about it.  The zone id is the *where*, the confidence is the *how sure*, and
/// the class is the *what*.
///
/// # Examples
///
/// ```
/// use cctv_detect::{Detection, Zone};
///
/// let zone = Zone::point(1, 1, 4);
/// let det = Detection::motion(zone, 0.87);
/// assert_eq!(det.confidence, 0.87);
/// assert!(det.is_motion());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Detection {
    /// The zone in which the object or motion was seen.
    pub zone: Zone,
    /// What was seen, short and stable (e.g. `person`, `motion`, `vehicle`).
    pub class: String,
    /// How sure the camera is, 0.0..1.0.
    pub confidence: f32,
    /// The bounding box in native units, if the device reported one.
    pub bbox: Option<Region>,
}

impl Detection {
    /// Build a motion detection in `zone` at `confidence`.
    pub fn motion(zone: Zone, confidence: f32) -> Self {
        Self {
            zone,
            class: "motion".into(),
            confidence: confidence.clamp(0.0, 1.0),
            bbox: None,
        }
    }

    /// Build an object detection in `zone`.
    pub fn object(zone: Zone, class: impl Into<String>, confidence: f32, bbox: Option<Region>) -> Self {
        Self {
            zone,
            class: class.into(),
            confidence: confidence.clamp(0.0, 1.0),
            bbox,
        }
    }

    /// `true` when the detection is motion, not an object.
    pub fn is_motion(&self) -> bool {
        self.class == "motion"
    }

    /// `true` when the detection is an object, not motion.
    pub fn is_object(&self) -> bool {
        !self.is_motion()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_motion_detection_fires_in_the_zone_it_was_given() {
        let zone = Zone::point(1, 1, 3);
        let det = Detection::motion(zone, 0.9);
        assert_eq!(det.zone, zone);
        assert!(det.is_motion());
        assert!(!det.is_object());
    }

    #[test]
    fn an_object_detection_carries_bounded_box_and_confidence() {
        let zone = Zone::rectangle(0.0, 0.0, 4, 4);
        let det = Detection::object(zone, "person", 0.81, Some(Region::new(1.0, 1.0, 2.0, 2.0)));
        assert_eq!(det.class, "person");
        assert!((det.confidence - 0.81).abs() < f32::EPSILON);
        assert!(det.is_object());
    }

    #[test]
    fn confidence_is_clamped_to_unit_interval() {
        let low = Detection::motion(Zone::point(0, 0, 1), -0.5);
        let high = Detection::motion(Zone::point(0, 0, 1), 2.0);
        assert!((low.confidence - 0.0).abs() < f32::EPSILON);
        assert!((high.confidence - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_point_zone_is_fully_specified_by_top_left_and_size() {
        let z = Zone::point(2, 3, 7);
        assert_eq!(z.kind, RegionKind::Point);
        assert_eq!(z.top, 2.0);
        assert_eq!(z.left, 3.0);
        assert_eq!(z.size, 7.0);
    }
}
