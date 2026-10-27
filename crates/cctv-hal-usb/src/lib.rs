//! `cctv-hal-usb` — USB HAL abstraction and device enumeration.
//!
//! Provides canned USB descriptor parsing, device class classification (UVC video,
//! UAC audio, composite, HID, vendor-specific), and USB bus sweep abstractions.

#![forbid(unsafe_code)]

pub mod descriptor;
pub mod sweep;

pub use descriptor::{class_codes, UsbDeviceDescriptor, UsbInterfaceDescriptor};
pub use sweep::{UsbEnumerator, UsbSweepResult};
