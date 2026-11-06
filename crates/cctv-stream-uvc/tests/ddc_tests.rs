//! Tests for DDC/CI monitor control in cctv-stream-uvc.

#![cfg(feature = "ddc")]

use cctv_core::{Capability, CapabilitySet};
use cctv_stream_uvc::ddc::*;

#[test]
fn test_ddc_packet_checksum() {
    // Checksum for write request: dest 0x6E, payload [0x51, 0x84, 0x03, 0x10, 0x00, 0x32] (val 50)
    let pkt = build_set_vcp_packet(0x10, 50);
    assert_eq!(pkt.len(), 7);
    assert_eq!(pkt[0], 0x51);
    assert_eq!(pkt[1], 0x84);
    assert_eq!(pkt[2], 0x03);
    assert_eq!(pkt[3], 0x10);
    assert_eq!(pkt[4], 0x00);
    assert_eq!(pkt[5], 50);

    let expected_chk = compute_checksum(0x6E, &pkt[..6]);
    assert_eq!(pkt[6], expected_chk);
}

#[test]
fn test_write_brightness_50_read_back_on_two_monitors() {
    let mut mon1 = DdcController::new(
        MockI2cBus::new(),
        vec![InputSource::Hdmi1, InputSource::Hdmi2],
    );
    let mut mon2 = DdcController::new(
        MockI2cBus::new(),
        vec![InputSource::Hdmi1, InputSource::Hdmi2],
    );
    let caps = CapabilitySet::new();

    mon1.set_brightness(50, &caps).expect("set b50 mon1");
    assert_eq!(mon1.get_brightness().expect("get b50 mon1"), 50);

    mon2.set_brightness(50, &caps).expect("set b50 mon2");
    assert_eq!(mon2.get_brightness().expect("get b50 mon2"), 50);

    mon1.set_input_source(InputSource::Hdmi1, &caps)
        .expect("set hdmi1 mon1");
    assert_eq!(
        mon1.get_input_source().expect("get hdmi1 mon1"),
        InputSource::Hdmi1
    );
}

#[test]
fn test_monitor_without_ddc_returns_i2c_unavailable() {
    let mut bus = MockI2cBus::new();
    bus.set_unavailable(true);
    let mut mon = DdcController::new(bus, vec![InputSource::Hdmi1]);
    let caps = CapabilitySet::new();

    assert_eq!(mon.get_brightness().unwrap_err(), DdcError::I2cUnavailable);
    assert_eq!(
        mon.set_brightness(50, &caps).unwrap_err(),
        DdcError::I2cUnavailable
    );
}

#[test]
fn test_linux_i2c_bus_nonexistent_returns_i2c_unavailable() {
    let bus = LinuxI2cBus::open("/dev/i2c-999");
    assert_eq!(bus.unwrap_err(), DdcError::I2cUnavailable);
}

#[test]
fn test_security_power_mode_write_refused_without_cap() {
    let mut mon = DdcController::new(MockI2cBus::new(), vec![InputSource::Hdmi1]);
    let caps_without = CapabilitySet::new();
    let mut caps_with = CapabilitySet::new();
    caps_with.insert(Capability::DisplayPowerWrite);

    assert_eq!(
        mon.set_power_mode(PowerMode::Standby, &caps_without)
            .unwrap_err(),
        DdcError::PermissionDenied
    );

    assert!(mon.set_power_mode(PowerMode::Standby, &caps_with).is_ok());
    assert_eq!(mon.get_power_mode().unwrap(), PowerMode::Standby);
}

#[test]
fn test_allowlist_input_source_validation() {
    let mut mon = DdcController::new(MockI2cBus::new(), vec![InputSource::Hdmi1]);
    let caps = CapabilitySet::new();

    assert!(mon.set_input_source(InputSource::Hdmi1, &caps).is_ok());
    assert_eq!(
        mon.set_input_source(InputSource::DisplayPort, &caps)
            .unwrap_err(),
        DdcError::UnsupportedInputSource(InputSource::DisplayPort.as_u16())
    );
}

#[test]
fn test_display_profile_application() {
    let mut mon = DdcController::new(
        MockI2cBus::new(),
        vec![InputSource::Hdmi1, InputSource::Hdmi2],
    );
    let mut caps = CapabilitySet::new();
    caps.insert(Capability::DisplayPowerWrite);

    mon.apply_profile(&DisplayProfile::night(), &caps)
        .expect("apply night profile");
    assert_eq!(mon.get_brightness().unwrap(), 30);
    assert_eq!(mon.get_input_source().unwrap(), InputSource::Hdmi2);

    mon.apply_profile(&DisplayProfile::day(), &caps)
        .expect("apply day profile");
    assert_eq!(mon.get_brightness().unwrap(), 80);
    assert_eq!(mon.get_input_source().unwrap(), InputSource::Hdmi1);
}
