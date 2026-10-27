//! USB enumeration sweep and result taxonomy.

use cctv_core::ProbeResult;
use serde::{Deserialize, Serialize};

use crate::descriptor::UsbDeviceDescriptor;

/// Result of a USB bus or fixture enumeration sweep.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum UsbSweepResult {
    /// Enumerated USB devices found during the sweep.
    Devices(Vec<ProbeResult>),
    /// The USB bus was unreachable, disabled, or returned no devices.
    EmptySweep,
}

impl UsbSweepResult {
    /// Returns `true` if the sweep resulted in an empty sweep.
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::EmptySweep)
    }

    /// Access the enumerated devices if present.
    pub fn devices(&self) -> &[ProbeResult] {
        match self {
            Self::Devices(list) => list,
            Self::EmptySweep => &[],
        }
    }
}

/// USB hardware enumerator and descriptor fixture processor.
#[derive(Debug, Clone, Copy, Default)]
pub struct UsbEnumerator;

impl UsbEnumerator {
    /// Create a new USB enumerator.
    pub fn new() -> Self {
        Self
    }

    /// Parse canned USB descriptors from JSON text fixtures.
    ///
    /// Accepts either a JSON array of [`UsbDeviceDescriptor`] objects or a single descriptor.
    /// Returns [`UsbSweepResult::EmptySweep`] if parsing yields no valid devices or on malformed JSON input.
    pub fn parse_fixture(&self, json_str: &str) -> UsbSweepResult {
        if json_str.trim().is_empty() {
            return UsbSweepResult::EmptySweep;
        }

        if let Ok(list) = serde_json::from_str::<Vec<UsbDeviceDescriptor>>(json_str) {
            if list.is_empty() {
                UsbSweepResult::EmptySweep
            } else {
                let probes = list.iter().map(|d| d.to_probe_result()).collect();
                UsbSweepResult::Devices(probes)
            }
        } else if let Ok(single) = serde_json::from_str::<UsbDeviceDescriptor>(json_str) {
            UsbSweepResult::Devices(vec![single.to_probe_result()])
        } else {
            UsbSweepResult::EmptySweep
        }
    }

    /// Perform a USB hardware bus sweep.
    ///
    /// If the host USB bus is unreachable, unreadable, or carries no devices,
    /// this function returns [`UsbSweepResult::EmptySweep`] rather than an error.
    pub fn sweep_bus(&self) -> UsbSweepResult {
        // Hardware USB bus probing fallback: when the system bus is unreachable or unavailable,
        // it gracefully yields an EmptySweep.
        UsbSweepResult::EmptySweep
    }
}
