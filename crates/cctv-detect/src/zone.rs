//! Zones: the boundaries a camera's detection monitor fires inside.
//!
//! A zone is a named, machine-readable window on a camera's field of view.  Two
//! shapes are supported:
//!
//! - `point` — a centered box of a fixed size (the camera's native resolution),
//!   for cameras whose analytics report a centred rectangle.
//! - `rectangle` — a normalised window, 0.0..1.0 in both axes, for devices that
//!   expose their zone grid.
//!
//! Zones are **value types**: they are published by one driver, adopted by the
//! fleet, and updated by the application without any allocator.

#![forbid(unsafe_code)]

/// One axis of a region.
pub type Region = (f32, f32, f32, f32);

/// What shape a zone is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RegionKind {
    /// A centred box of `size` at `(left, top)`, in native pixels.
    Point,
    /// A normalised window from 0.0..1.0 in both axes.
    Rectangle,
}

/// A named rectangle on the camera's output, in its native coordinate space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zone {
    /// The kind of the zone.
    pub kind: RegionKind,
    /// Top-left corner in native units.
    pub left: f32,
    /// Top-left corner in native units.
    pub top: f32,
    /// Width, in native units (point) or 0.0..1.0 (rectangle).
    pub width: f32,
    /// Height, in native units (point) or 0.0..1.0 (rectangle).
    pub height: f32,
}

impl Zone {
    /// Build a centred point zone.
    ///
    /// `left` / `top` name the zone's centre; `size` names the side length.
    /// The result is clamped to the camera's footprint so it never asks for
    /// more than the view carries.
    pub fn point(left: f32, top: f32, size: f32) -> Self {
        Self {
            kind: RegionKind::Point,
            left: left.clamp(0.0, f32::MAX),
            top: top.clamp(0.0, f32::MAX),
            width: size.max(0.0),
            height: size.max(0.0),
        }
    }

    /// Build a normalised rectangle zone.
    ///
    /// `left` / `top` are the top-left corner in 0.0..1.0; `width` / `height`
    /// are the extents, also 0.0..1.0.
    pub fn rectangle(left: f32, top: f32, width: f32, height: f32) -> Self {
        let width = width.clamp(0.0, 1.0);
        let height = height.clamp(0.0, 1.0);
        let left = left.clamp(0.0, 1.0).min(1.0 - width);
        let top = top.clamp(0.0, 1.0).min(1.0 - height);
        Self {
            kind: RegionKind::Rectangle,
            left,
            top,
            width,
            height,
        }
    }

    /// The width, in native units (point) or 0.0..1.0 (rectangle).
    pub fn width(self) -> f32 {
        self.width
    }

    /// The height, in native units (point) or 0.0..1.0 (rectangle).
    pub fn height(self) -> f32 {
        self.height
    }
}

/// A zone's configuration for the zone-motion monitor.
#[derive(Debug, Clone, PartialEq)]
pub struct ZoneConfig {
    /// The zone itself.
    pub zone: Zone,
    /// The minimum confidence the monitor reports a detection for.
    pub threshold: f32,
    /// The minimum number of detections in the window before it fires.
    pub min_detections: u32,
    /// The window length in seconds before a firing clears.
    pub window_s: u64,
}

impl ZoneConfig {
    /// A sensible default for a zone with a single motion region.
    pub fn default_for(zone: Zone, threshold: f32) -> Self {
        Self {
            zone,
            threshold: threshold.clamp(0.0, 1.0),
            min_detections: 1,
            window_s: 30,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_zone_is_centred_and_non_negative() {
        let z = Zone::point(2.0, 3.0, 10.0);
        assert_eq!((z.left, z.top, z.width, z.height), (2.0, 3.0, 10.0, 10.0));
        assert_eq!(z.kind, RegionKind::Point);
    }

    #[test]
    fn a_point_zone_clamps_size_to_zero() {
        let z = Zone::point(0.0, 0.0, -5.0);
        assert_eq!(z.width, 0.0);
        assert_eq!(z.height, 0.0);
    }

    #[test]
    fn a_rectangle_is_normalised_and_clamped() {
        let z = Zone::rectangle(0.25, 0.25, 1.5, 1.5);
        assert!((z.left - 0.25).abs() < f32::EPSILON);
        assert!((z.top - 0.25).abs() < f32::EPSILON);
        assert!((z.width - 1.0).abs() < f32::EPSILON);
        assert!((z.height - 1.0).abs() < f32::EPSILON);
        assert_eq!(z.kind, RegionKind::Rectangle);
    }

    #[test]
    fn a_rectangle_with_zero_width_is_empty() {
        let z = Zone::rectangle(0.0, 0.0, 0.0, 1.0);
        assert_eq!(z.width, 0.0);
        assert_eq!(z.height, 1.0);
    }

    #[test]
    fn a_zone_config_uses_the_given_threshold() {
        let z = ZoneConfig::default_for(Zone::point(0.0, 0.0, 4.0), 0.5);
        assert!((z.threshold - 0.5).abs() < f32::EPSILON);
        assert_eq!(z.min_detections, 1);
        assert_eq!(z.window_s, 30);
    }

    #[test]
    fn a_zone_config_clamps_the_threshold() {
        let z = ZoneConfig::default_for(Zone::point(0.0, 0.0, 1.0), 3.0);
        assert!((z.threshold - 1.0).abs() < f32::EPSILON);
    }
}
