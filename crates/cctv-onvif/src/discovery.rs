//! ONVIF discovery: WS-Discovery sweeps delegated to `cctv-discovery`.
//!
//! [`cctv_discovery`]'s merged [`Probe`](cctv_discovery::Probe) sweep answers
//! both SSDP and WS-Discovery — and SSDP finds every UPnP box on the LAN, not
//! just cameras, so that sweep cannot move into this crate.  What this module
//! adds is the ONVIF half: the same multicast exchange over the same
//! [`Datagram`] seam, gated by ONVIF evidence (an `onvif://` scope or an
//! ONVIF `Types` entry) and by the presence of an HTTP(S) `XAddr` — because
//! without the control-service URL there is nothing to open an
//! [`OnvifSession`](crate::OnvifSession) against.
//!
//! WS-Discovery only, by design: the ONVIF Core Specification requires every
//! device to answer a WS-Discovery `Probe`, so a device that never answers one
//! is not discoverable as ONVIF no matter what it says over SSDP.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use cctv_discovery::probe::{camera_from_probe_match, message_id};
use cctv_discovery::{wsd, Datagram, DiscoveredCamera, UdpTransport};

/// A sweep that finds its protocol's devices on the local network.
///
/// Object-safe (`Send + Sync`, no generics) so the facade can hold it behind an
/// `Arc<dyn Discoverable>`, mirroring `tv-rs`'s `SsdpDiscovery`.
pub trait Discoverable: Send + Sync {
    /// Sweep over an injected transport — the test seam, no sockets.
    fn discover_with(&self, net: &dyn Datagram) -> Vec<DiscoveredCamera>;

    /// Sweep the real network over UDP multicast.
    ///
    /// An unreachable network yields an empty sweep, never an error.
    fn discover(&self) -> Vec<DiscoveredCamera> {
        self.discover_with(&UdpTransport::new())
    }
}

/// WS-Discovery sweep filtered to ONVIF-capable devices.
#[derive(Debug, Clone, Copy)]
pub struct OnvifDiscovery {
    timeout: Duration,
}

impl OnvifDiscovery {
    /// A sweep with a 1.5 s collection window (the same default `Probe` uses).
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_millis(1_500),
        }
    }

    /// Override how long the sweep collects replies.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl Default for OnvifDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl Discoverable for OnvifDiscovery {
    fn discover_with(&self, net: &dyn Datagram) -> Vec<DiscoveredCamera> {
        let group = SocketAddr::from(([239, 255, 255, 250], wsd::PORT));
        let request = wsd::probe(&message_id());

        // First sighting of each identity wins, ordered by identity — the same
        // dedupe contract as the shared `Probe` sweep.
        let mut found: BTreeMap<String, DiscoveredCamera> = BTreeMap::new();
        for datagram in net.exchange(group, request.as_bytes(), self.timeout) {
            let document = String::from_utf8_lossy(&datagram);
            for matched in wsd::parse_probe_matches(&document) {
                if !is_onvif(&matched) {
                    continue;
                }
                // The converter keeps a match whose only identity is the
                // endpoint, but an ONVIF device we cannot address is not
                // discoverable as ONVIF: require the HTTP(S) XAddr.
                if let Some(camera) = camera_from_probe_match(&matched) {
                    if camera.service_url.is_some() {
                        found.entry(camera.id.clone()).or_insert(camera);
                    }
                }
            }
        }
        found.into_values().collect()
    }
}

/// ONVIF evidence in a `ProbeMatch`: an `onvif://` scope, or an ONVIF type.
///
/// A WS-Discovery responder without either is some other Windows/UPnP thing
/// that happened to hear the probe.
fn is_onvif(matched: &wsd::ProbeMatch) -> bool {
    matched
        .scopes
        .iter()
        .any(|scope| scope.starts_with("onvif://"))
        || matched.types.contains("NetworkVideoTransmitter")
        || matched.types.contains("tds:Device")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Canned WS-Discovery replies plus a record of what was sent.
    struct Fake {
        replies: Vec<String>,
        sent_to: std::sync::Mutex<Vec<(SocketAddr, Vec<u8>)>>,
    }

    impl Fake {
        fn with(replies: &[&str]) -> Self {
            Self {
                replies: replies.iter().map(|s| (*s).to_string()).collect(),
                sent_to: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl Datagram for Fake {
        fn exchange(&self, group: SocketAddr, request: &[u8], _timeout: Duration) -> Vec<Vec<u8>> {
            self.sent_to.lock().unwrap().push((group, request.to_vec()));
            self.replies.iter().map(|r| r.as_bytes().to_vec()).collect()
        }
    }

    /// One envelope with an ONVIF camera, a non-ONVIF responder and a device
    /// with no HTTP(S) XAddr.
    const MIXED: &str = r#"<s:Envelope><s:Body><d:ProbeMatches>
        <d:ProbeMatch>
            <w:EndpointReference><w:Address>urn:uuid:cam-1</w:Address></w:EndpointReference>
            <d:Types>tds:Device dn:NetworkVideoTransmitter</d:Types>
            <d:Scopes>onvif://www.onvif.org/name/Front%20Door onvif://www.onvif.org/manufacturer/ACME onvif://www.onvif.org/model/PTZ-9000</d:Scopes>
            <d:XAddrs>http://192.168.1.64/onvif/device_service</d:XAddrs>
        </d:ProbeMatch>
        <d:ProbeMatch>
            <w:EndpointReference><w:Address>urn:uuid:printer-9</w:Address></w:EndpointReference>
            <d:Types>pd:Printer</d:Types>
            <d:Scopes>urn:uuid:printer-9</d:Scopes>
            <d:XAddrs>http://192.168.1.77:60000/printer</d:XAddrs>
        </d:ProbeMatch>
        <d:ProbeMatch>
            <w:EndpointReference><w:Address>urn:uuid:cam-2</w:Address></w:EndpointReference>
            <d:Types>tds:Device</d:Types>
            <d:Scopes>onvif://www.onvif.org/model/Bare</d:Scopes>
            <d:XAddrs>urn:u:g:service:device_service</d:XAddrs>
        </d:ProbeMatch>
    </d:ProbeMatches></s:Body></s:Envelope>"#;

    #[test]
    fn an_onvif_device_is_discovered_with_its_control_service_url() {
        let found = OnvifDiscovery::new().discover_with(&Fake::with(&[MIXED]));

        assert_eq!(found.len(), 1, "only the addressable ONVIF device");
        let camera = &found[0];
        assert_eq!(camera.id, "urn:uuid:cam-1");
        assert_eq!(
            camera.service_url.as_deref(),
            Some("http://192.168.1.64/onvif/device_service")
        );
        assert_eq!(camera.probe.vendor, "ACME");
        assert_eq!(camera.probe.model, "PTZ-9000");
        assert_eq!(camera.probe.ip.as_deref(), Some("192.168.1.64"));
    }

    #[test]
    fn a_responder_without_onvif_evidence_is_not_a_camera() {
        // MIXED minus the ONVIF matches: only the printer answers.
        let printer_only = MIXED
            .replace("onvif://www.onvif.org/name/Front%20Door onvif://www.onvif.org/manufacturer/ACME onvif://www.onvif.org/model/PTZ-9000", "urn:x")
            .replace("tds:Device dn:NetworkVideoTransmitter", "pd:Printer")
            .replace("<d:Scopes>onvif://www.onvif.org/model/Bare</d:Scopes>", "<d:Scopes>urn:y</d:Scopes>")
            .replace("<d:Types>tds:Device</d:Types>", "<d:Types>pd:Printer</d:Types>");

        let found = OnvifDiscovery::new().discover_with(&Fake::with(&[&printer_only]));
        assert!(
            found.is_empty(),
            "a printer is not an ONVIF camera: {found:?}"
        );
    }

    #[test]
    fn an_onvif_device_without_an_http_xaddr_is_not_discoverable() {
        // Full ONVIF evidence, but only a urn: XAddr — there is no address to
        // open a session against, so the device must not come back.
        let unaddressable = r#"<s:Envelope><s:Body><d:ProbeMatches>
            <d:ProbeMatch>
                <w:EndpointReference><w:Address>urn:uuid:cam-2</w:Address></w:EndpointReference>
                <d:Types>tds:Device dn:NetworkVideoTransmitter</d:Types>
                <d:Scopes>onvif://www.onvif.org/model/Bare</d:Scopes>
                <d:XAddrs>urn:u:g:service:device_service</d:XAddrs>
            </d:ProbeMatch>
        </d:ProbeMatches></s:Body></s:Envelope>"#;

        let found = OnvifDiscovery::new().discover_with(&Fake::with(&[unaddressable]));
        assert!(
            found.is_empty(),
            "ONVIF evidence without an HTTP XAddr is not discoverable: {found:?}"
        );
    }

    #[test]
    fn repeated_sightings_collapse_to_one_entry() {
        let found = OnvifDiscovery::new().discover_with(&Fake::with(&[MIXED, MIXED]));
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn an_silent_network_yields_an_empty_sweep() {
        let found = OnvifDiscovery::new().discover_with(&Fake::with(&[]));
        assert!(found.is_empty());
    }

    #[test]
    fn the_probe_goes_to_ws_discovery_as_a_uuid_scoped_envelope() {
        let fake = Fake::with(&[MIXED]);
        let _ = OnvifDiscovery::new().discover_with(&fake);

        let sent = fake.sent_to.lock().unwrap();
        assert_eq!(sent.len(), 1, "exactly one multicast exchange");
        let (group, request) = &sent[0];
        assert_eq!(group.port(), wsd::PORT, "UDP 3702, not SSDP's 1900");
        let request = String::from_utf8_lossy(request);
        assert!(request.contains("<d:Probe/>"), "a bare WS-Discovery Probe");
        let message_id = request
            .split("<w:MessageID>")
            .nth(1)
            .and_then(|rest| rest.split("</w:MessageID>").next())
            .expect("a MessageID");
        assert!(message_id.starts_with("urn:uuid:"), "{message_id}");
    }

    #[test]
    fn the_default_sweep_timeout_is_the_probe_window() {
        assert_eq!(OnvifDiscovery::new().timeout, Duration::from_millis(1_500));
        let custom = OnvifDiscovery::new().with_timeout(Duration::from_secs(4));
        assert_eq!(custom.timeout, Duration::from_secs(4));
    }
}
