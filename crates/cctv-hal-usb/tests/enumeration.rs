//! Integration tests for USB descriptor enumeration and classification.

use cctv_core::{DeviceClass, LinkKind};
use cctv_hal_usb::{UsbEnumerator, UsbSweepResult};

#[test]
fn uvc_camera_fixture_yields_usb_link_and_video_classification() {
    let json = include_str!("../fixtures/uvc_camera.json");
    let enumerator = UsbEnumerator::new();
    let result = enumerator.parse_fixture(json);

    assert!(!result.is_empty());
    let devices = result.devices();
    assert_eq!(devices.len(), 1);

    let probe = &devices[0];
    assert_eq!(probe.vendor, "Logitech");
    assert_eq!(probe.model, "HD Pro Webcam C920");
    assert_eq!(probe.link_kind, LinkKind::Usb);
    assert_eq!(probe.device_class, Some(DeviceClass::Video));
    assert_eq!(probe.mac.as_deref(), Some("9D1F5B42"));
    assert_eq!(probe.stream_url.as_deref(), Some("bus-1-port-2"));
}

#[test]
fn audio_mic_fixture_yields_usb_link_and_audio_classification() {
    let json = include_str!("../fixtures/audio_mic.json");
    let enumerator = UsbEnumerator::new();
    let result = enumerator.parse_fixture(json);

    let devices = result.devices();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].link_kind, LinkKind::Usb);
    assert_eq!(devices[0].device_class, Some(DeviceClass::Audio));
}

#[test]
fn composite_webcam_fixture_yields_usb_link_and_composite_classification() {
    let json = include_str!("../fixtures/composite_webcam.json");
    let enumerator = UsbEnumerator::new();
    let result = enumerator.parse_fixture(json);

    let devices = result.devices();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].link_kind, LinkKind::Usb);
    assert_eq!(devices[0].device_class, Some(DeviceClass::Composite));
}

#[test]
fn hid_device_fixture_yields_usb_link_and_hid_classification() {
    let json = include_str!("../fixtures/hid_device.json");
    let enumerator = UsbEnumerator::new();
    let result = enumerator.parse_fixture(json);

    let devices = result.devices();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].link_kind, LinkKind::Usb);
    assert_eq!(devices[0].device_class, Some(DeviceClass::Hid));
}

#[test]
fn vendor_device_fixture_yields_usb_link_and_vendor_classification() {
    let json = include_str!("../fixtures/vendor_device.json");
    let enumerator = UsbEnumerator::new();
    let result = enumerator.parse_fixture(json);

    let devices = result.devices();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].link_kind, LinkKind::Usb);
    assert_eq!(devices[0].device_class, Some(DeviceClass::VendorSpecific));
}

#[test]
fn unreachable_bus_returns_empty_sweep() {
    let enumerator = UsbEnumerator::new();

    // Invalid JSON fixture
    let empty_json = enumerator.parse_fixture("invalid json");
    assert_eq!(empty_json, UsbSweepResult::EmptySweep);

    // Empty list fixture
    let empty_list = enumerator.parse_fixture("[]");
    assert_eq!(empty_list, UsbSweepResult::EmptySweep);

    // Hardware bus sweep on unreachable bus
    let bus_sweep = enumerator.sweep_bus();
    assert_eq!(bus_sweep, UsbSweepResult::EmptySweep);
}
