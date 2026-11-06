//! Ambient light monitoring and display profile policy automation.
//!
//! Evaluates ambient illuminance (lux) readings and automatically applies
//! appropriate display profiles (e.g. night profile when lux < 100), recording
//! decisions into the agent audit trail.

use crate::audit::Audit;
use cctv_core::CapabilitySet;
use cctv_stream_uvc::ddc::{DdcController, DdcError, DisplayProfile, I2cBus};

/// Ambient light threshold (lux) below which night display profile is triggered.
pub const NIGHT_LUX_THRESHOLD: f32 = 100.0;

/// Ambient light policy evaluator for edge displays.
pub struct AmbientDisplayPolicy;

impl AmbientDisplayPolicy {
    /// Evaluate ambient illuminance (in lux).
    /// If ambient light < 100 lux, applies the night profile (brightness 30, HDMI2)
    /// and logs the action to `audit`.
    /// Otherwise, applies the day profile (brightness 80, HDMI1) and logs the action to `audit`.
    pub fn evaluate<B: I2cBus>(
        display_id: &str,
        ambient_lux: f32,
        controller: &mut DdcController<B>,
        caller_caps: &CapabilitySet,
        audit: &mut Audit,
    ) -> Result<DisplayProfile, DdcError> {
        let (profile, note) = if ambient_lux < NIGHT_LUX_THRESHOLD {
            (
                DisplayProfile::night(),
                format!(
                    "ambient light {:.1} lux < {:.1} lux: applied night profile (brightness 30)",
                    ambient_lux, NIGHT_LUX_THRESHOLD
                ),
            )
        } else {
            (
                DisplayProfile::day(),
                format!(
                    "ambient light {:.1} lux >= {:.1} lux: applied day profile (brightness 80)",
                    ambient_lux, NIGHT_LUX_THRESHOLD
                ),
            )
        };

        controller.apply_profile(&profile, caller_caps)?;

        // Log decision to audit trail (grade 1 for minor automated remediation/policy adjustment)
        audit.log(display_id.to_string(), &note, 1);

        Ok(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cctv_core::Capability;
    use cctv_stream_uvc::ddc::{InputSource, MockI2cBus, PowerMode};

    #[test]
    fn test_ambient_light_below_100_lux_applies_night_profile_and_audits() {
        let bus = MockI2cBus::new();
        let mut controller = DdcController::new(
            bus,
            vec![
                InputSource::Hdmi1,
                InputSource::Hdmi2,
                InputSource::DisplayPort,
            ],
        );
        let mut caps = CapabilitySet::new();
        caps.insert(Capability::DisplayPowerWrite);

        let mut audit = Audit::new();

        // 45 lux < 100 lux -> Night profile
        let profile =
            AmbientDisplayPolicy::evaluate("monitor-0", 45.0, &mut controller, &caps, &mut audit)
                .expect("applied profile");

        assert_eq!(profile.name, "night");
        assert_eq!(controller.get_brightness().unwrap(), 30);
        assert_eq!(controller.get_input_source().unwrap(), InputSource::Hdmi2);
        assert_eq!(controller.get_power_mode().unwrap(), PowerMode::On);

        // Verify audit log
        assert_eq!(audit.len(), 1);
        let entry = &audit.entries()[0];
        assert_eq!(entry.camera_id, "monitor-0");
        assert!(entry.what.contains("night profile (brightness 30)"));
    }

    #[test]
    fn test_ambient_light_above_100_lux_applies_day_profile_and_audits() {
        let bus = MockI2cBus::new();
        let mut controller = DdcController::new(
            bus,
            vec![
                InputSource::Hdmi1,
                InputSource::Hdmi2,
                InputSource::DisplayPort,
            ],
        );
        let mut caps = CapabilitySet::new();
        caps.insert(Capability::DisplayPowerWrite);

        let mut audit = Audit::new();

        // 350 lux >= 100 lux -> Day profile
        let profile =
            AmbientDisplayPolicy::evaluate("monitor-1", 350.0, &mut controller, &caps, &mut audit)
                .expect("applied profile");

        assert_eq!(profile.name, "day");
        assert_eq!(controller.get_brightness().unwrap(), 80);
        assert_eq!(controller.get_input_source().unwrap(), InputSource::Hdmi1);

        assert_eq!(audit.len(), 1);
        let entry = &audit.entries()[0];
        assert_eq!(entry.camera_id, "monitor-1");
        assert!(entry.what.contains("day profile (brightness 80)"));
    }
}
