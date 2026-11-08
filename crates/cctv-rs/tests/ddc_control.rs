//! Acceptance and integration test suite for DDC/CI monitor control.
//!
//! Acceptance criteria:
//! 1. Write brightness 50 -> read back 50 on two real monitors; write input HDMI1 -> read back HDMI1.
//! 2. Monitor without DDC returns I2cUnavailable, no panic.
//! 3. Security: Refuse to write VCP 0xD6 (power) unless the caller has capability display.power.write.
//! 4. Allowlist: Validate input source against an allowlist of values the monitor reports as supported.
//! 5. Ambient light < 100 lux -> agent applies night profile (brightness 30) and logs to audit.

#![cfg(feature = "ddc")]

use cctv_core::{Capability, CapabilitySet};
use cctv_rs::{
    AmbientDisplayPolicy, Audit, DdcController, DdcError, InputSource, MockI2cBus, PowerMode,
    VcpCode,
};

#[test]
fn test_write_brightness_50_read_back_on_two_monitors_and_input_hdmi1() {
    // Monitor 1
    let bus1 = MockI2cBus::new();
    let mut monitor1 = DdcController::new(
        bus1,
        vec![
            InputSource::Hdmi1,
            InputSource::Hdmi2,
            InputSource::DisplayPort,
        ],
    );

    // Monitor 2
    let bus2 = MockI2cBus::new();
    let mut monitor2 = DdcController::new(
        bus2,
        vec![
            InputSource::Hdmi1,
            InputSource::Hdmi2,
            InputSource::DisplayPort,
        ],
    );

    let caps = CapabilitySet::new();

    // Write brightness 50 -> read back 50 on monitor 1
    monitor1
        .set_brightness(50, &caps)
        .expect("write brightness 50");
    let b1 = monitor1.get_brightness().expect("read brightness");
    assert_eq!(b1, 50);

    // Write brightness 50 -> read back 50 on monitor 2
    monitor2
        .set_brightness(50, &caps)
        .expect("write brightness 50");
    let b2 = monitor2.get_brightness().expect("read brightness");
    assert_eq!(b2, 50);

    // Write input HDMI1 -> read back HDMI1 on monitor 1
    monitor1
        .set_input_source(InputSource::Hdmi1, &caps)
        .expect("set input hdmi1");
    let in1 = monitor1.get_input_source().expect("read input hdmi1");
    assert_eq!(in1, InputSource::Hdmi1);

    // Write input HDMI2 -> read back HDMI2 on monitor 2
    monitor2
        .set_input_source(InputSource::Hdmi2, &caps)
        .expect("set input hdmi2");
    let in2 = monitor2.get_input_source().expect("read input hdmi2");
    assert_eq!(in2, InputSource::Hdmi2);
}

#[test]
fn test_monitor_without_ddc_returns_i2c_unavailable_no_panic() {
    let mut bus = MockI2cBus::new();
    bus.set_unavailable(true);

    let mut controller = DdcController::new(bus, vec![InputSource::Hdmi1]);
    let caps = CapabilitySet::new();

    // Read brightness returns I2cUnavailable
    let res_read = controller.get_brightness();
    assert_eq!(res_read.unwrap_err(), DdcError::I2cUnavailable);

    // Write brightness returns I2cUnavailable
    let res_write = controller.set_brightness(60, &caps);
    assert_eq!(res_write.unwrap_err(), DdcError::I2cUnavailable);

    // Read VCP returns I2cUnavailable
    let res_vcp = controller.get_vcp(VcpCode::Brightness);
    assert_eq!(res_vcp.unwrap_err(), DdcError::I2cUnavailable);

    // None panicked
}

#[test]
fn test_security_refuses_vcp_d6_without_display_power_write_capability() {
    let bus = MockI2cBus::new();
    let mut controller = DdcController::new(bus, vec![InputSource::Hdmi1]);

    // Caller without display.power.write capability
    let caps_without_power = CapabilitySet::new();

    let err = controller
        .set_power_mode(PowerMode::Standby, &caps_without_power)
        .unwrap_err();
    assert_eq!(err, DdcError::PermissionDenied);

    let raw_err = controller
        .set_vcp(VcpCode::PowerMode, 4, &caps_without_power)
        .unwrap_err();
    assert_eq!(raw_err, DdcError::PermissionDenied);

    // Caller WITH display.power.write capability
    let mut caps_with_power = CapabilitySet::new();
    caps_with_power.insert(Capability::DisplayPowerWrite);

    controller
        .set_power_mode(PowerMode::Standby, &caps_with_power)
        .expect("power write succeeds with capability");
    assert_eq!(controller.get_power_mode().unwrap(), PowerMode::Standby);
}

#[test]
fn test_input_source_validation_against_supported_allowlist() {
    let bus = MockI2cBus::new();
    // Only HDMI1 and DisplayPort supported on this monitor
    let mut controller =
        DdcController::new(bus, vec![InputSource::Hdmi1, InputSource::DisplayPort]);
    let caps = CapabilitySet::new();

    // Setting HDMI1 succeeds
    assert!(controller
        .set_input_source(InputSource::Hdmi1, &caps)
        .is_ok());

    // Setting HDMI2 fails because it's not in the allowlist
    let err = controller
        .set_input_source(InputSource::Hdmi2, &caps)
        .unwrap_err();
    assert_eq!(
        err,
        DdcError::UnsupportedInputSource(InputSource::Hdmi2.as_u16())
    );
}

#[test]
fn test_ambient_light_below_100_lux_applies_night_profile_and_logs_audit() {
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

    // Initial brightness is 50
    assert_eq!(controller.get_brightness().unwrap(), 50);

    // Ambient light 40 lux (< 100 lux)
    let applied =
        AmbientDisplayPolicy::evaluate("display-primary", 40.0, &mut controller, &caps, &mut audit)
            .expect("evaluated policy");

    assert_eq!(applied.name, "night");
    assert_eq!(controller.get_brightness().unwrap(), 30);
    assert_eq!(controller.get_input_source().unwrap(), InputSource::Hdmi2);

    // Confirm audit log
    assert_eq!(audit.len(), 1);
    let entry = &audit.entries()[0];
    assert_eq!(entry.camera_id, "display-primary");
    assert!(entry.what.contains("ambient light 40.0 lux < 100.0 lux"));
    assert!(entry.what.contains("applied night profile (brightness 30)"));
}
