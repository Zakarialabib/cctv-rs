//! Passive mDNS TXT record parsing and device identification.

use std::collections::HashMap;

use crate::error::ActionCamError;

/// Discovered Action Camera identity details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionCamIdentity {
    /// Device model string (e.g. "Insta360 X4").
    pub model: String,
    /// Manufacturer name.
    pub manufacturer: String,
    /// Device serial number if present in TXT record.
    pub serial_number: Option<String>,
    /// API version supported by camera HTTP endpoint.
    pub api_version: Option<String>,
    /// IP address of discovered device.
    pub ip: String,
    /// HTTP port for status queries.
    pub port: u16,
}

/// Identify an Insta360 X4 camera passively from mDNS TXT record key-value pairs.
pub fn identify_insta360_x4(
    txt_map: &HashMap<String, String>,
    ip: &str,
    port: u16,
) -> Result<ActionCamIdentity, ActionCamError> {
    let get_key = |key: &str| -> Option<String> {
        txt_map
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.clone())
    };

    let manufacturer = get_key("manufacturer")
        .or_else(|| get_key("vendor"))
        .unwrap_or_else(|| "Insta360".to_string());

    let model = get_key("model")
        .or_else(|| get_key("md"))
        .or_else(|| get_key("camera_model"))
        .ok_or_else(|| ActionCamError::NotAnInsta360("Missing model in TXT record".into()))?;

    let model_lower = model.to_lowercase();
    if !model_lower.contains("insta360") && !manufacturer.to_lowercase().contains("insta360") {
        return Err(ActionCamError::NotAnInsta360(format!(
            "Manufacturer/model '{manufacturer} {model}' is not Insta360"
        )));
    }

    if !model_lower.contains("x4") {
        return Err(ActionCamError::NotAnInsta360(format!(
            "Model '{model}' is not an Insta360 X4"
        )));
    }

    let serial_number = get_key("serial")
        .or_else(|| get_key("sn"))
        .or_else(|| get_key("serial_number"));

    let api_version = get_key("api_version")
        .or_else(|| get_key("ver"))
        .or_else(|| get_key("api"));

    Ok(ActionCamIdentity {
        model: "Insta360 X4".to_string(),
        manufacturer: "Insta360".to_string(),
        serial_number,
        api_version,
        ip: ip.to_string(),
        port,
    })
}
