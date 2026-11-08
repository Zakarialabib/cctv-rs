//! The pure half of PTZ: a device-model [`PtzCommand`] mapped onto the
//! platform's discrete direction + speed vocabulary.
//!
//! Ezviz moves on eight compass directions plus zoom and focus, at three speeds,
//! and requires an explicit `stop` naming the direction that was started.  A
//! [`PtzCommand`] carries continuous velocities instead, so this module is the
//! adapter between the two — and it is pure, so the mapping is unit-tested
//! without a network.

use cctv_core::PtzCommand;
use ezviz_openapi::ptz::{PtzDirection, PtzSpeed};

/// Velocities below this are treated as "no movement on this axis", so a nearly
/// vertical command does not silently become a diagonal one.
const DEAD_ZONE: f32 = 0.2;

/// Map continuous velocities onto `(direction, speed)`.
///
/// `None` when every axis is inside the dead zone — the camera would not move,
/// so there is nothing to send.  Zoom wins over pan / tilt when both are asked
/// for, because the platform cannot do both in one call.
pub fn continuous_to_direction(pan: f32, tilt: f32, zoom: f32) -> Option<(PtzDirection, PtzSpeed)> {
    let pan = axis(pan);
    let tilt = axis(tilt);
    let zoom = axis(zoom);
    let magnitude = pan.abs().max(tilt.abs()).max(zoom.abs());
    if magnitude == 0.0 {
        return None;
    }

    let direction = if zoom != 0.0 && zoom.abs() >= pan.abs().max(tilt.abs()) {
        if zoom > 0.0 {
            PtzDirection::ZoomIn
        } else {
            PtzDirection::ZoomOut
        }
    } else {
        // `f32::signum` returns `1.0` for `+0.0`, which would turn a purely
        // vertical command into a diagonal one; compare explicitly instead.
        let sign = |v: f32| {
            if v > 0.0 {
                1
            } else if v < 0.0 {
                -1
            } else {
                0
            }
        };
        match (sign(pan), sign(tilt)) {
            (0, 1) => PtzDirection::Up,
            (0, -1) => PtzDirection::Down,
            (-1, 0) => PtzDirection::Left,
            (1, 0) => PtzDirection::Right,
            (-1, 1) => PtzDirection::UpperLeft,
            (-1, -1) => PtzDirection::LowerLeft,
            (1, 1) => PtzDirection::UpperRight,
            (1, -1) => PtzDirection::LowerRight,
            _ => return None,
        }
    };
    Some((direction, speed_for(magnitude)))
}

/// The three platform speeds, split evenly across the unit range.
pub fn speed_for(magnitude: f32) -> PtzSpeed {
    if magnitude < 1.0 / 3.0 {
        PtzSpeed::Slow
    } else if magnitude < 2.0 / 3.0 {
        PtzSpeed::Medium
    } else {
        PtzSpeed::Fast
    }
}

/// Clamp to `-1..=1`, zero anything non-finite, and apply the dead zone.
fn axis(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    let clamped = value.clamp(-1.0, 1.0);
    if clamped.abs() < DEAD_ZONE {
        0.0
    } else {
        clamped
    }
}

/// Translate a device-model command into a platform direction.
///
/// `None` when the platform cannot express the command: it has no absolute move,
/// and `Stop` needs the direction that was started, which only the caller knows.
pub fn direction_for(command: PtzCommand) -> Option<(PtzDirection, PtzSpeed)> {
    match command {
        PtzCommand::Continuous { pan, tilt, zoom } => continuous_to_direction(pan, tilt, zoom),
        PtzCommand::Absolute { .. } | PtzCommand::Stop => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cardinal_directions_map_one_to_one() {
        assert_eq!(
            continuous_to_direction(0.0, 1.0, 0.0).unwrap().0,
            PtzDirection::Up
        );
        assert_eq!(
            continuous_to_direction(0.0, -1.0, 0.0).unwrap().0,
            PtzDirection::Down
        );
        assert_eq!(
            continuous_to_direction(-1.0, 0.0, 0.0).unwrap().0,
            PtzDirection::Left
        );
        assert_eq!(
            continuous_to_direction(1.0, 0.0, 0.0).unwrap().0,
            PtzDirection::Right
        );
    }

    #[test]
    fn the_diagonals_map_to_their_quadrants() {
        assert_eq!(
            continuous_to_direction(-0.8, 0.8, 0.0).unwrap().0,
            PtzDirection::UpperLeft
        );
        assert_eq!(
            continuous_to_direction(-0.8, -0.8, 0.0).unwrap().0,
            PtzDirection::LowerLeft
        );
        assert_eq!(
            continuous_to_direction(0.8, 0.8, 0.0).unwrap().0,
            PtzDirection::UpperRight
        );
        assert_eq!(
            continuous_to_direction(0.8, -0.8, 0.0).unwrap().0,
            PtzDirection::LowerRight
        );
    }

    #[test]
    fn a_small_axis_is_inside_the_dead_zone_so_it_stays_cardinal() {
        // 0.1 is below the dead zone: this is "up", not "upper left".
        assert_eq!(
            continuous_to_direction(-0.1, 0.9, 0.0).unwrap().0,
            PtzDirection::Up
        );
    }

    #[test]
    fn zoom_wins_when_it_is_the_dominant_axis() {
        assert_eq!(
            continuous_to_direction(0.0, 0.0, 0.9).unwrap().0,
            PtzDirection::ZoomIn
        );
        assert_eq!(
            continuous_to_direction(0.0, 0.0, -0.9).unwrap().0,
            PtzDirection::ZoomOut
        );
        // A tie goes to zoom, and is documented as doing so.
        assert_eq!(
            continuous_to_direction(0.5, 0.0, 0.5).unwrap().0,
            PtzDirection::ZoomIn
        );
    }

    #[test]
    fn speed_splits_the_unit_range_in_three() {
        assert_eq!(speed_for(0.1), PtzSpeed::Slow);
        assert_eq!(speed_for(0.5), PtzSpeed::Medium);
        assert_eq!(speed_for(1.0), PtzSpeed::Fast);
        assert_eq!(
            continuous_to_direction(0.25, 0.0, 0.0).unwrap().1,
            PtzSpeed::Slow
        );
        assert_eq!(
            continuous_to_direction(0.99, 0.0, 0.0).unwrap().1,
            PtzSpeed::Fast
        );
    }

    #[test]
    fn nothing_asked_for_moves_nothing() {
        assert_eq!(continuous_to_direction(0.0, 0.0, 0.0), None);
        assert_eq!(continuous_to_direction(0.05, -0.05, 0.0), None);
        // Non-finite input is zeroed rather than propagated to the device.
        assert_eq!(continuous_to_direction(f32::NAN, f32::INFINITY, 0.0), None);
        assert_eq!(
            continuous_to_direction(f32::NAN, 0.9, 0.0).unwrap().0,
            PtzDirection::Up
        );
    }

    #[test]
    fn out_of_range_velocities_are_clamped_not_rejected() {
        assert_eq!(
            continuous_to_direction(9.0, 0.0, 0.0).unwrap(),
            (PtzDirection::Right, PtzSpeed::Fast)
        );
        assert_eq!(
            continuous_to_direction(-9.0, 0.0, 0.0).unwrap(),
            (PtzDirection::Left, PtzSpeed::Fast)
        );
    }

    #[test]
    fn absolute_and_stop_are_not_expressible_here() {
        assert_eq!(
            direction_for(PtzCommand::Absolute {
                pan: 0.5,
                tilt: 0.0,
                zoom: 0.0
            }),
            None
        );
        assert_eq!(direction_for(PtzCommand::Stop), None);
        assert_eq!(
            direction_for(PtzCommand::Continuous {
                pan: 0.0,
                tilt: 1.0,
                zoom: 0.0
            })
            .unwrap()
            .0,
            PtzDirection::Up
        );
    }
}
