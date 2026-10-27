//! USB descriptor definitions and hardware classification logic.

use cctv_core::{DeviceClass, LinkKind, ProbeResult};
use serde::{Deserialize, Serialize};

/// Interface descriptor representation from USB enumeration or JSON fixtures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsbInterfaceDescriptor {
    /// Interface number (`bInterfaceNumber`).
    #[serde(alias = "bInterfaceNumber", default)]
    pub interface_number: u8,
    /// Interface class code (`bInterfaceClass`).
    #[serde(alias = "bInterfaceClass", default)]
    pub interface_class: u8,
    /// Interface subclass code (`bInterfaceSubClass`).
    #[serde(alias = "bInterfaceSubClass", default)]
    pub interface_subclass: u8,
    /// Interface protocol code (`bInterfaceProtocol`).
    #[serde(alias = "bInterfaceProtocol", default)]
    pub interface_protocol: u8,
    /// Optional interface name string descriptor.
    #[serde(alias = "interfaceName", default)]
    pub interface_name: Option<String>,
}

/// Device descriptor representation from USB enumeration or JSON fixtures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsbDeviceDescriptor {
    /// Vendor ID (`idVendor`).
    #[serde(alias = "idVendor", alias = "vendor_id", default)]
    pub vendor_id: u16,
    /// Product ID (`idProduct`).
    #[serde(alias = "idProduct", alias = "product_id", default)]
    pub product_id: u16,
    /// Device class code (`bDeviceClass`).
    #[serde(alias = "bDeviceClass", alias = "device_class", default)]
    pub device_class: u8,
    /// Device subclass code (`bDeviceSubClass`).
    #[serde(alias = "bDeviceSubClass", alias = "device_subclass", default)]
    pub device_subclass: u8,
    /// Device protocol code (`bDeviceProtocol`).
    #[serde(alias = "bDeviceProtocol", alias = "device_protocol", default)]
    pub device_protocol: u8,
    /// Reported vendor / manufacturer name string.
    #[serde(alias = "vendorName", alias = "manufacturer", default)]
    pub vendor_name: Option<String>,
    /// Reported product / model name string.
    #[serde(alias = "productName", alias = "product", default)]
    pub product_name: Option<String>,
    /// Device serial number string descriptor.
    #[serde(alias = "serialNumber", alias = "serial_number", default)]
    pub serial_number: Option<String>,
    /// Bus address or path (e.g. `"bus-1-port-2"`).
    #[serde(alias = "busAddress", alias = "bus_address", default)]
    pub bus_address: Option<String>,
    /// List of USB interface descriptors.
    #[serde(default)]
    pub interfaces: Vec<UsbInterfaceDescriptor>,
}

/// USB class code constants according to USB-IF specifications.
pub mod class_codes {
    /// Audio Device Class (UAC).
    pub const AUDIO: u8 = 0x01;
    /// Human Interface Device Class (HID).
    pub const HID: u8 = 0x03;
    /// Video Device Class (UVC).
    pub const VIDEO: u8 = 0x0E;
    /// Miscellaneous Device Class (e.g., Composite IAD).
    pub const MISC: u8 = 0xEF;
    /// Vendor Specific Class.
    pub const VENDOR_SPECIFIC: u8 = 0xFF;
}

impl UsbDeviceDescriptor {
    /// Classify the device descriptor into a [`DeviceClass`].
    pub fn classify(&self) -> DeviceClass {
        let mut has_video = self.device_class == class_codes::VIDEO;
        let mut has_audio = self.device_class == class_codes::AUDIO;
        let mut has_hid = self.device_class == class_codes::HID;
        let mut has_vendor = self.device_class == class_codes::VENDOR_SPECIFIC;

        for iface in &self.interfaces {
            match iface.interface_class {
                class_codes::VIDEO => has_video = true,
                class_codes::AUDIO => has_audio = true,
                class_codes::HID => has_hid = true,
                class_codes::VENDOR_SPECIFIC => has_vendor = true,
                _ => {}
            }
        }

        if has_video && has_audio {
            DeviceClass::Composite
        } else if has_video {
            DeviceClass::Video
        } else if has_audio {
            DeviceClass::Audio
        } else if has_hid {
            DeviceClass::Hid
        } else if has_vendor {
            DeviceClass::VendorSpecific
        } else {
            DeviceClass::Unknown
        }
    }

    /// Convert this USB descriptor into a core [`ProbeResult`].
    pub fn to_probe_result(&self) -> ProbeResult {
        let class = self.classify();
        let vendor = self
            .vendor_name
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("USB Vendor 0x{:04x}", self.vendor_id));
        let model = self
            .product_name
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("USB Device 0x{:04x}", self.product_id));

        ProbeResult {
            vendor,
            model,
            firmware: None,
            ip: None,
            mac: self.serial_number.clone(),
            stream_url: self.bus_address.clone(),
            ptz_supported: false,
            ai_supported: false,
            link_kind: LinkKind::Usb,
            device_class: Some(class),
        }
    }
}
