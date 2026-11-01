//! Imperative camera commands.
//!
//! Direct commands targeted at a camera device bypassing declarative state reconciliation,
//! such as PTZ pulses, instantaneous snapshot triggers, and clock synchronization.

use crate::error::CoreError;
use crate::prelude::*;

#[cfg(feature = "serde-sync")]
use serde::{Deserialize, Serialize};

/// High-priority imperative command directed to a camera.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde-sync", serde(rename_all = "snake_case", tag = "kind"))]
pub enum ImperativeCameraCommand {
    /// Time-bounded PTZ pulse command.
    PtzPulse {
        /// Normalized pan velocity in `-1.0..=1.0`.
        pan: f32,
        /// Normalized tilt velocity in `-1.0..=1.0`.
        tilt: f32,
        /// Normalized zoom velocity in `-1.0..=1.0`.
        zoom: f32,
        /// Duration of the pulse in milliseconds, clamped to `100..=1000`.
        ttl_ms: u16,
    },
    /// Stop all current PTZ motion immediately.
    PtzStop,
    /// Move the camera to a specified PTZ preset.
    PtzGotoPreset {
        /// Unique preset identifier or token.
        preset_token: String,
    },
    /// Trigger an instant snapshot capture on the camera.
    TriggerInstantSnapshot,
    /// Synchronize the camera internal clock to UTC.
    SyncClockToUtc,
}

impl ImperativeCameraCommand {
    /// Construct a validated `PtzPulse` command.
    ///
    /// Validates `pan`, `tilt`, and `zoom` to lie within `-1.0..=1.0`.
    /// Clamps `ttl_ms` to `100..=1000`.
    pub fn ptz_pulse(pan: f32, tilt: f32, zoom: f32, ttl_ms: u16) -> Result<Self, CoreError> {
        let valid_axis = |v: f32| (-1.0..=1.0).contains(&v);
        if !valid_axis(pan) || !valid_axis(tilt) || !valid_axis(zoom) {
            return Err(CoreError::Other(
                "PTZ pulse velocity out of bounds [-1.0, 1.0]".to_string(),
            ));
        }

        let clamped_ttl = ttl_ms.clamp(100, 1000);
        Ok(Self::PtzPulse {
            pan,
            tilt,
            zoom,
            ttl_ms: clamped_ttl,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ptz_pulse_valid() {
        let cmd = ImperativeCameraCommand::ptz_pulse(0.5, -0.5, 0.0, 500).unwrap();
        match cmd {
            ImperativeCameraCommand::PtzPulse {
                pan,
                tilt,
                zoom,
                ttl_ms,
            } => {
                assert_eq!(pan, 0.5);
                assert_eq!(tilt, -0.5);
                assert_eq!(zoom, 0.0);
                assert_eq!(ttl_ms, 500);
            }
            _ => panic!("unexpected command variant"),
        }
    }

    #[test]
    fn test_ptz_pulse_clamps_ttl() {
        let cmd_low = ImperativeCameraCommand::ptz_pulse(0.0, 0.0, 0.0, 10).unwrap();
        if let ImperativeCameraCommand::PtzPulse { ttl_ms, .. } = cmd_low {
            assert_eq!(ttl_ms, 100);
        } else {
            panic!("unexpected variant");
        }

        let cmd_high = ImperativeCameraCommand::ptz_pulse(0.0, 0.0, 0.0, 5000).unwrap();
        if let ImperativeCameraCommand::PtzPulse { ttl_ms, .. } = cmd_high {
            assert_eq!(ttl_ms, 1000);
        } else {
            panic!("unexpected variant");
        }
    }

    #[test]
    fn test_ptz_pulse_out_of_bounds() {
        assert!(ImperativeCameraCommand::ptz_pulse(1.5, 0.0, 0.0, 200).is_err());
        assert!(ImperativeCameraCommand::ptz_pulse(0.0, -1.1, 0.0, 200).is_err());
        assert!(ImperativeCameraCommand::ptz_pulse(0.0, 0.0, 2.0, 200).is_err());
    }

    #[test]
    fn test_other_variants() {
        let stop = ImperativeCameraCommand::PtzStop;
        assert_eq!(stop, ImperativeCameraCommand::PtzStop);

        let preset = ImperativeCameraCommand::PtzGotoPreset {
            preset_token: "preset_1".to_string(),
        };
        assert_eq!(
            preset,
            ImperativeCameraCommand::PtzGotoPreset {
                preset_token: "preset_1".to_string()
            }
        );

        let snap = ImperativeCameraCommand::TriggerInstantSnapshot;
        assert_eq!(snap, ImperativeCameraCommand::TriggerInstantSnapshot);

        let sync = ImperativeCameraCommand::SyncClockToUtc;
        assert_eq!(sync, ImperativeCameraCommand::SyncClockToUtc);
    }
}
