//! WS-Discovery — ONVIF's discovery protocol (SOAP over UDP multicast).
//!
//! Pure: the `Probe` envelope is built here and `ProbeMatches` are read with
//! `cctv-transport`'s prefix-agnostic XML scanner; the UDP exchange lives in
//! [`crate::probe`].
//!
//! ONVIF devices answer a WS-Discovery `Probe` with their endpoint address,
//! their service URLs (`XAddr`) and their `onvif://` scopes — richer than SSDP,
//! and specific to cameras.

use cctv_transport::xml;

/// The WS-Discovery multicast group.
pub const MULTICAST_ADDR: &str = "239.255.255.250";
/// The WS-Discovery port.
pub const PORT: u16 = 3702;

const DISCOVERY_NS: &str = "http://schemas.xmlsoap.org/ws/2005/04/discovery";
const SOAP_NS: &str = "http://www.w3.org/2003/05/soap-envelope";
const ADDRESSING_NS: &str = "http://schemas.xmlsoap.org/ws/2004/08/addressing";

/// Build a WS-Discovery `Probe` envelope.
///
/// `message_id` must be a fresh `urn:uuid:` per sweep so a device can tell two
/// sweeps apart; [`crate::probe`] mints one.
pub fn probe(message_id: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <e:Envelope xmlns:e=\"{SOAP_NS}\" xmlns:w=\"{ADDRESSING_NS}\" xmlns:d=\"{DISCOVERY_NS}\">\
         <e:Header>\
         <w:Action>http://schemas.xmlsoap.org/ws/2005/04/discovery/Probe</w:Action>\
         <w:MessageID>{id}</w:MessageID>\
         <w:To>urn:schemas-xmlsoap-org:ws:2005:04:discovery</w:To>\
         </e:Header>\
         <e:Body><d:Probe/></e:Body>\
         </e:Envelope>",
        id = xml::escape(message_id)
    )
}

/// One `ProbeMatch` from a `ProbeMatches` reply.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProbeMatch {
    /// `EndpointReference/Address` — the device's stable identity.
    pub endpoint: String,
    /// `Types`, e.g. `tds:Device`.
    pub types: String,
    /// The `onvif://` scopes the device advertises.
    pub scopes: Vec<String>,
    /// The service URLs the device advertises.
    pub xaddrs: Vec<String>,
}

impl ProbeMatch {
    /// The value of the first `onvif://.../<kind>/<value>` scope.
    ///
    /// `kind` is a scope keyword — `name`, `hardware`, `location`, `type`,
    /// `manufacturer`, `model`.  The keyword is matched case-insensitively and
    /// the value is returned verbatim.
    pub fn scope(&self, kind: &str) -> Option<&str> {
        let needle = format!("/{}/", kind.to_ascii_lowercase());
        self.scopes.iter().find_map(|scope| {
            scope
                .to_ascii_lowercase()
                .find(&needle)
                .map(|at| &scope[at + needle.len()..])
        })
    }

    /// The first `XAddr` that is an HTTP(S) URL — the control-service endpoint.
    pub fn service_url(&self) -> Option<&str> {
        self.xaddrs
            .iter()
            .find(|url| url.starts_with("http://") || url.starts_with("https://"))
            .map(String::as_str)
    }
}

/// Parse every `ProbeMatch` in a `ProbeMatches` envelope.
///
/// Prefix-agnostic and total: an unrelated or malformed document yields an empty
/// list, and a match with no endpoint still comes back with its fields empty.
pub fn parse_probe_matches(document: &str) -> Vec<ProbeMatch> {
    xml::elements(document, "ProbeMatch")
        .into_iter()
        .map(|element| ProbeMatch {
            endpoint: xml::element_text(element.body, "Address").unwrap_or_default(),
            types: xml::element_text(element.body, "Types").unwrap_or_default(),
            scopes: xml::element_text(element.body, "Scopes")
                .map(|text| words(&text))
                .unwrap_or_default(),
            xaddrs: xml::element_text(element.body, "XAddrs")
                .map(|text| words(&text))
                .unwrap_or_default(),
        })
        .collect()
}

/// Split a space-separated list, dropping empties.
fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MATCHES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope" xmlns:d="http://schemas.xmlsoap.org/ws/2005/04/discovery" xmlns:w="http://schemas.xmlsoap.org/ws/2004/08/addressing">
  <s:Body><d:ProbeMatches><d:ProbeMatches>
    <d:ProbeMatch>
      <w:EndpointReference><w:Address>urn:uuid:3fa1fe68-b915-4053-a3e1-1000b1a2b3c4</w:Address></w:EndpointReference>
      <d:Types>tds:Device</d:Types>
      <d:Scopes>onvif://www.onvif.org/type/Network_Video_Transmitter onvif://www.onvif.org/name/Front%20Door onvif://www.onvif.org/hardware/DS-2CD2143G0</d:Scopes>
      <d:XAddrs>http://192.168.1.64/onvif/device_service http://[fe80::1]/onvif/device_service</d:XAddrs>
      <d:MetadataVersion>1</d:MetadataVersion>
    </d:ProbeMatch>
    <d:ProbeMatch>
      <w:EndpointReference><w:Address>urn:uuid:0f0f0f0f-1111-2222-3333-444455556666</w:Address></w:EndpointReference>
      <d:Types>tds:Device</d:Types>
      <d:Scopes>onvif://www.onvif.org/manufacturer/ACME onvif://www.onvif.org/model/PTZ-9000</d:Scopes>
      <d:XAddrs>https://10.0.0.9/onvif/device_service</d:XAddrs>
    </d:ProbeMatch>
  </d:ProbeMatches></d:ProbeMatches></s:Body>
</s:Envelope>"#;

    #[test]
    fn probe_carries_the_action_and_an_escaped_message_id() {
        let envelope = probe("urn:uuid:a&b");
        assert!(envelope.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(envelope.contains(
            "<w:Action>http://schemas.xmlsoap.org/ws/2005/04/discovery/Probe</w:Action>"
        ));
        assert!(envelope.contains("<w:MessageID>urn:uuid:a&amp;b</w:MessageID>"));
        assert!(envelope.contains("<d:Probe/>"));
        assert!(!envelope.contains("a&b"), "the id must be escaped");
    }

    #[test]
    fn probe_matches_are_read_one_by_one() {
        let matches = parse_probe_matches(MATCHES);
        assert_eq!(matches.len(), 2);
        assert_eq!(
            matches[0].endpoint,
            "urn:uuid:3fa1fe68-b915-4053-a3e1-1000b1a2b3c4"
        );
        assert_eq!(matches[0].types, "tds:Device");
        assert_eq!(matches[0].scopes.len(), 3);
        assert_eq!(matches[0].xaddrs.len(), 2);
        assert_eq!(
            matches[1].endpoint,
            "urn:uuid:0f0f0f0f-1111-2222-3333-444455556666"
        );
        assert_eq!(matches[1].scopes.len(), 2);
    }

    #[test]
    fn scopes_are_matched_by_kind_and_the_service_url_is_the_first_http_xaddr() {
        let matches = parse_probe_matches(MATCHES);
        assert_eq!(matches[0].scope("name"), Some("Front%20Door"));
        assert_eq!(matches[0].scope("hardware"), Some("DS-2CD2143G0"));
        assert_eq!(matches[0].scope("type"), Some("Network_Video_Transmitter"));
        assert_eq!(matches[0].scope("manufacturer"), None);
        assert_eq!(matches[1].scope("manufacturer"), Some("ACME"));
        assert_eq!(matches[1].scope("model"), Some("PTZ-9000"));
        assert_eq!(
            matches[0].service_url(),
            Some("http://192.168.1.64/onvif/device_service")
        );
        assert_eq!(
            matches[1].service_url(),
            Some("https://10.0.0.9/onvif/device_service")
        );
    }

    #[test]
    fn an_unrelated_or_empty_document_yields_nothing() {
        assert!(parse_probe_matches("").is_empty());
        assert!(parse_probe_matches("<s:Envelope/>").is_empty());
        assert!(parse_probe_matches("not xml at all").is_empty());
        // A `ProbeMatch` with no fields still comes back, empty.
        let bare = parse_probe_matches("<d:ProbeMatch/>");
        assert_eq!(bare.len(), 1);
        assert!(bare[0].endpoint.is_empty());
        assert_eq!(bare[0].service_url(), None);
    }
}
