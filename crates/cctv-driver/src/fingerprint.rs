//! Device fingerprint matching, RTSP path templates, and driver priority.
//!
//! §2.1: The 5-step ladder relies on matching fingerprint rules against discovered
//! camera probe details (manufacturer, model, MAC prefixes, ports) and trying
//! prioritized RTSP templates for non-ONVIF devices.

use cctv_core::ProbeResult;

/// Well-known priority bands for driver selection.
///
/// Priority values are unsigned 16-bit integers, where **lower numeric values
/// indicate higher priority**:
/// - `SPECIFIC`: Vendor-specific native protocols (e.g., Hikvision native, Ezviz, Axis)
/// - `OEM`: OEM / white-label platform adapters (e.g., Xiongmai / Sofia / Tuya)
/// - `FALLBACK`: Generic standards-based fallback (e.g., ONVIF Profile S/T/G)
/// - `GENERIC`: Last-resort generic RTSP probe template fallback
pub mod priority {
    /// Vendor-specific proprietary driver priority (highest precedence).
    pub const SPECIFIC: u16 = 100;

    /// White-label / OEM platform driver priority.
    pub const OEM: u16 = 500;

    /// Standards-based fallback priority (ONVIF).
    pub const FALLBACK: u16 = 1000;

    /// Generic RTSP probe template priority (lowest precedence).
    pub const GENERIC: u16 = 5000;
}

/// An RTSP path template for non-ONVIF cameras or fallback streaming.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtspTemplate {
    /// Template pattern for the main/high-res stream (e.g., `"/h264Preview_01_main"`, `"/live/ch0"`).
    pub main_path: String,
    /// Template pattern for the sub/low-res stream (e.g., `"/h264Preview_01_sub"`, `"/live/ch1"`).
    pub sub_path: Option<String>,
    /// Default RTSP port (typically 554, or vendor-specific e.g. 8554, 10554).
    pub default_port: u16,
    /// Channel placeholder variable, if supported (e.g., `{channel}`).
    pub supports_channel_variable: bool,
}

impl RtspTemplate {
    /// Create a new RTSP path template.
    pub fn new(main_path: impl Into<String>, default_port: u16) -> Self {
        Self {
            main_path: main_path.into(),
            sub_path: None,
            default_port,
            supports_channel_variable: false,
        }
    }

    /// Set the sub-stream path template.
    pub fn with_sub_path(mut self, sub_path: impl Into<String>) -> Self {
        self.sub_path = Some(sub_path.into());
        self
    }

    /// Indicate whether `{channel}` parameter interpolation is supported.
    pub fn with_channel_variable(mut self, supported: bool) -> Self {
        self.supports_channel_variable = supported;
        self
    }

    /// Render the main stream path for a specific channel.
    pub fn render_main(&self, channel: u32) -> String {
        if self.supports_channel_variable {
            self.main_path.replace("{channel}", &channel.to_string())
        } else {
            self.main_path.clone()
        }
    }

    /// Render the sub stream path for a specific channel.
    pub fn render_sub(&self, channel: u32) -> Option<String> {
        self.sub_path.as_ref().map(|template| {
            if self.supports_channel_variable {
                template.replace("{channel}", &channel.to_string())
            } else {
                template.clone()
            }
        })
    }
}

/// A fingerprint rule for identifying camera vendors from discovery probe responses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FingerprintRule {
    /// Manufacturer / vendor name prefixes or substrings (case-insensitive).
    pub manufacturers: Vec<String>,
    /// Model name prefixes or substrings (case-insensitive).
    pub models: Vec<String>,
    /// MAC address OUI prefixes (e.g., `"44:19:b6"`, `"bc:54:51"`).
    pub mac_ouis: Vec<String>,
    /// Server header substrings from SSDP or HTTP responses.
    pub server_headers: Vec<String>,
    /// Driver key match if explicitly targeted.
    pub target_driver: Option<String>,
}

impl FingerprintRule {
    /// Create a new empty fingerprint rule.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a fingerprint matching a driver key.
    pub fn for_driver(driver_key: impl Into<String>) -> Self {
        Self {
            target_driver: Some(driver_key.into()),
            ..Self::default()
        }
    }

    /// Add a manufacturer substring to match against.
    pub fn with_manufacturer(mut self, manufacturer: impl Into<String>) -> Self {
        self.manufacturers.push(manufacturer.into().to_lowercase());
        self
    }

    /// Add a model substring to match against.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.models.push(model.into().to_lowercase());
        self
    }

    /// Add a MAC OUI prefix (e.g. `"44:19:b6"` or `"44-19-B6"`).
    pub fn with_mac_oui(mut self, mac_oui: impl Into<String>) -> Self {
        let normalized = mac_oui.into().to_lowercase().replace('-', ":");
        self.mac_ouis.push(normalized);
        self
    }

    /// Add a server header token to match against.
    pub fn with_server_header(mut self, header: impl Into<String>) -> Self {
        self.server_headers.push(header.into().to_lowercase());
        self
    }

    /// Check if a probe result satisfies this fingerprint rule.
    pub fn matches(&self, probe: &ProbeResult) -> bool {
        // If driver key is specified and matches vendor directly
        if let Some(target) = &self.target_driver {
            if probe.vendor.eq_ignore_ascii_case(target) {
                return true;
            }
        }

        let probe_vendor = probe.vendor.to_lowercase();
        let probe_model = probe.model.to_lowercase();

        // 1. Check manufacturers
        for m in &self.manufacturers {
            if !probe_vendor.is_empty() && probe_vendor.contains(m) {
                return true;
            }
        }

        // 2. Check models
        for m in &self.models {
            if !probe_model.is_empty() && probe_model.contains(m) {
                return true;
            }
        }

        // 3. Check MAC OUIs if available
        if let Some(mac) = &probe.mac {
            let normalized_mac = mac.to_lowercase().replace('-', ":");
            for oui in &self.mac_ouis {
                if normalized_mac.starts_with(oui) {
                    return true;
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtsp_template_rendering() {
        let template = RtspTemplate::new("/live/ch{channel}_main", 554)
            .with_sub_path("/live/ch{channel}_sub")
            .with_channel_variable(true);

        assert_eq!(template.render_main(1), "/live/ch1_main");
        assert_eq!(template.render_sub(1), Some("/live/ch1_sub".into()));
        assert_eq!(template.default_port, 554);
    }

    #[test]
    fn fingerprint_rule_matching() {
        let rule = FingerprintRule::new()
            .with_manufacturer("hikvision")
            .with_model("ds-2cd")
            .with_mac_oui("44:19:b6");

        let mut probe = ProbeResult {
            vendor: "Hikvision Digital Technology".to_string(),
            model: "DS-2CD2042WD-I".to_string(),
            firmware: None,
            ip: Some("192.168.1.64".into()),
            mac: Some("44-19-B6-AA-BB-CC".into()),
            stream_url: None,
            ptz_supported: false,
            ai_supported: false,
        };

        assert!(rule.matches(&probe));

        // Test vendor match alone
        probe.model = "Unknown".into();
        probe.mac = None;
        assert!(rule.matches(&probe));

        // Test model match alone
        probe.vendor = "OEM Corp".into();
        probe.model = "DS-2CD2143G0".into();
        assert!(rule.matches(&probe));

        // Test MAC OUI match alone
        probe.model = "Generic".into();
        probe.mac = Some("44:19:b6:11:22:33".into());
        assert!(rule.matches(&probe));

        // Non-match
        probe.mac = Some("00:11:22:33:44:55".into());
        assert!(!rule.matches(&probe));
    }
}
