//! Device management service: identity and service discovery.

use cctv_transport::xml;

use crate::error::OnvifError;

/// `GetDeviceInformation` body.
pub fn build_get_device_information() -> &'static str {
    "<tds:GetDeviceInformation/>"
}

/// `GetCapabilities` (all categories) body.
pub fn build_get_capabilities() -> &'static str {
    "<tds:GetCapabilities><tds:Category>All</tds:Category></tds:GetCapabilities>"
}

/// The `GetDeviceInformationResponse` fields.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceInformation {
    /// `Manufacturer`.
    pub manufacturer: String,
    /// `Model`.
    pub model: String,
    /// `FirmwareVersion`.
    pub firmware_version: Option<String>,
    /// `SerialNumber`.
    pub serial_number: Option<String>,
    /// `HardwareId`.
    pub hardware_id: Option<String>,
}

/// Parse a `GetDeviceInformationResponse`.
pub fn parse_device_information(body: &str) -> Result<DeviceInformation, OnvifError> {
    let scope = xml::element_body(body, "GetDeviceInformationResponse")
        .ok_or_else(|| OnvifError::Parse("no GetDeviceInformationResponse in the reply".into()))?;
    Ok(DeviceInformation {
        manufacturer: xml::element_text(scope, "Manufacturer").unwrap_or_default(),
        model: xml::element_text(scope, "Model").unwrap_or_default(),
        firmware_version: xml::element_text(scope, "FirmwareVersion"),
        serial_number: xml::element_text(scope, "SerialNumber"),
        hardware_id: xml::element_text(scope, "HardwareId"),
    })
}

/// Service endpoints advertised by `GetCapabilities`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Services {
    /// Media service `XAddr`.
    pub media: Option<String>,
    /// PTZ service `XAddr`; `None` for a fixed camera.
    pub ptz: Option<String>,
}

/// Parse a `GetCapabilitiesResponse`: the `XAddr` inside `Media` and `PTZ`.
pub fn parse_capabilities(body: &str) -> Result<Services, OnvifError> {
    let caps = xml::element_body(body, "Capabilities")
        .ok_or_else(|| OnvifError::Parse("no Capabilities in the reply".into()))?;
    let xaddr = |section: &str| {
        xml::element_body(caps, section)
            .and_then(|b| xml::element_text(b, "XAddr"))
            .filter(|u| !u.is_empty())
    };
    Ok(Services {
        media: xaddr("Media"),
        ptz: xaddr("PTZ"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICE_INFO: &str = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope" xmlns:tds="http://www.onvif.org/ver10/device/wsdl"><SOAP-ENV:Body><tds:GetDeviceInformationResponse>
        <tds:Manufacturer>ACME</tds:Manufacturer><tds:Model>PTZ-9000</tds:Model>
        <tds:FirmwareVersion>V5.6.3 build 200101</tds:FirmwareVersion>
        <tds:SerialNumber>SN123</tds:SerialNumber><tds:HardwareId>88</tds:HardwareId>
      </tds:GetDeviceInformationResponse></SOAP-ENV:Body></SOAP-ENV:Envelope>"#;

    #[test]
    fn device_information_is_read_field_by_field() {
        let d = parse_device_information(DEVICE_INFO).unwrap();
        assert_eq!(d.manufacturer, "ACME");
        assert_eq!(d.model, "PTZ-9000");
        assert_eq!(d.firmware_version.as_deref(), Some("V5.6.3 build 200101"));
        assert_eq!(d.serial_number.as_deref(), Some("SN123"));
    }

    #[test]
    fn a_reply_without_the_response_element_is_a_parse_error() {
        assert!(parse_device_information("<Envelope/>").is_err());
    }

    #[test]
    fn capabilities_yield_media_and_ptz_xaddrs() {
        let body = r#"<tds:GetCapabilitiesResponse><tds:Capabilities>
            <tt:Device><tt:XAddr>http://cam/onvif/device_service</tt:XAddr></tt:Device>
            <tt:Media><tt:XAddr>http://cam/onvif/Media</tt:XAddr></tt:Media>
            <tt:PTZ><tt:XAddr>http://cam/onvif/PTZ</tt:XAddr></tt:PTZ>
          </tds:Capabilities></tds:GetCapabilitiesResponse>"#;
        let s = parse_capabilities(body).unwrap();
        assert_eq!(s.media.as_deref(), Some("http://cam/onvif/Media"));
        assert_eq!(s.ptz.as_deref(), Some("http://cam/onvif/PTZ"));
    }

    #[test]
    fn a_fixed_camera_has_no_ptz_service() {
        let body = "<Capabilities><Media><XAddr>http://cam/m</XAddr></Media></Capabilities>";
        assert_eq!(parse_capabilities(body).unwrap().ptz, None);
    }
}
