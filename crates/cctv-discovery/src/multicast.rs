//! Step 1 Multicast Window: WS-Discovery & Hikvision SADP probe inquiry.
//!
//! Sends WS-Discovery (`239.255.255.250:3702`) and Hikvision SADP XML inquiry
//! (`239.255.255.250:37020`, `<Probe><Uuid>...</Uuid><Types>inquiry</Types></Probe>`).
//! Counts `total_datagrams_received`. If `0` after 2s, returns
//! [`DiscoveryOutcome::MulticastSilence`].

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use cctv_core::ProbeResult;
use cctv_transport::xml;

use crate::probe::{message_id, Datagram, DiscoveredCamera, UdpTransport};
use crate::wsd;

/// Multicast group for WS-Discovery and SADP probes.
pub const MULTICAST_IP: [u8; 4] = [239, 255, 255, 250];

/// Port for WS-Discovery probes.
pub const WSD_PORT: u16 = 3702;

/// Port for Hikvision SADP probes.
pub const SADP_PORT: u16 = 37020;

/// Default multicast collection window (2 seconds).
pub const MULTICAST_WINDOW: Duration = Duration::from_secs(2);

/// Outcome of a multicast discovery window.
#[derive(Debug, Clone, PartialEq)]
pub enum DiscoveryOutcome {
    /// Cameras discovered from multicast datagrams.
    Discovered(Vec<DiscoveredCamera>),
    /// Zero datagrams received across the multicast window (C1 signal).
    MulticastSilence,
}

/// Build a Hikvision SADP XML inquiry probe payload.
///
/// Format: `<Probe><Uuid>{uuid}</Uuid><Types>inquiry</Types></Probe>`
pub fn sadp_probe(uuid: &str) -> String {
    format!(
        "<Probe><Uuid>{}</Uuid><Types>inquiry</Types></Probe>",
        xml::escape(uuid)
    )
}

/// Parsed SADP inquiry response fields.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SadpReply {
    /// Device model.
    pub model: Option<String>,
    /// Device MAC address.
    pub mac: Option<String>,
    /// IPv4 address.
    pub ip: Option<String>,
    /// Software / firmware version.
    pub firmware: Option<String>,
    /// Device serial number / UUID.
    pub uuid: Option<String>,
}

/// Parse an XML SADP response datagram.
pub fn parse_sadp_reply(xml: &str) -> Option<SadpReply> {
    if !xml.contains("<ProbeMatch")
        && !xml.contains("<Probe")
        && !xml.contains("<Device")
        && !xml.contains("DeviceType")
        && !xml.contains("MAC")
        && !xml.contains("IPv4Address")
    {
        return None;
    }

    let model = xml::element_text(xml, "DeviceType")
        .or_else(|| xml::element_text(xml, "DeviceModel"))
        .or_else(|| xml::element_text(xml, "Model"));

    let mac = xml::element_text(xml, "MAC").or_else(|| xml::element_text(xml, "MACAddress"));

    let ip = xml::element_text(xml, "IPv4Address").or_else(|| xml::element_text(xml, "IPAddress"));

    let firmware = xml::element_text(xml, "SoftwareVersion")
        .or_else(|| xml::element_text(xml, "FirmwareVersion"));

    let uuid = xml::element_text(xml, "Uuid")
        .or_else(|| xml::element_text(xml, "DeviceDescription"))
        .or_else(|| xml::element_text(xml, "DeviceSN"));

    if model.is_none() && mac.is_none() && ip.is_none() {
        return None;
    }

    Some(SadpReply {
        model,
        mac,
        ip,
        firmware,
        uuid,
    })
}

/// Turn an SADP reply into a [`DiscoveredCamera`].
pub fn camera_from_sadp(reply: &SadpReply) -> Option<DiscoveredCamera> {
    let id = reply
        .uuid
        .clone()
        .or_else(|| reply.mac.clone())
        .or_else(|| reply.ip.clone())?;

    Some(DiscoveredCamera {
        id,
        probe: ProbeResult {
            vendor: "Hikvision".to_string(),
            model: reply.model.clone().unwrap_or_default(),
            firmware: reply.firmware.clone(),
            ip: reply.ip.clone(),
            mac: reply.mac.clone(),
            stream_url: None,
            ptz_supported: false,
            ai_supported: false,
        },
        service_url: reply
            .ip
            .as_deref()
            .map(|ip| format!("http://{ip}/onvif/device_service")),
    })
}

/// Run a Step 1 multicast discovery window over real UDP sockets.
pub fn scan_multicast() -> DiscoveryOutcome {
    scan_multicast_with(&UdpTransport::new(), MULTICAST_WINDOW)
}

/// Run a Step 1 multicast discovery window over an injected transport.
pub fn scan_multicast_with(net: &dyn Datagram, window: Duration) -> DiscoveryOutcome {
    let mut total_datagrams_received = 0usize;
    let mut discovered = Vec::new();

    // 1. WS-Discovery Probe on 239.255.255.250:3702
    let wsd_group = SocketAddr::from((MULTICAST_IP, WSD_PORT));
    let wsd_req = wsd::probe(&message_id());
    let wsd_replies = net.exchange(wsd_group, wsd_req.as_bytes(), window);
    total_datagrams_received += wsd_replies.len();
    for d in wsd_replies {
        let text = String::from_utf8_lossy(&d);
        for matched in wsd::parse_probe_matches(&text) {
            if let Some(cam) = crate::probe::camera_from_probe_match(&matched) {
                discovered.push(cam);
            }
        }
    }

    // 2. Hikvision SADP Inquiry on 239.255.255.250:37020
    let sadp_group = SocketAddr::from((MULTICAST_IP, SADP_PORT));
    let sadp_req = sadp_probe(&message_id());
    let sadp_replies = net.exchange(sadp_group, sadp_req.as_bytes(), window);
    total_datagrams_received += sadp_replies.len();
    for d in sadp_replies {
        let text = String::from_utf8_lossy(&d);
        if let Some(reply) = parse_sadp_reply(&text) {
            if let Some(cam) = camera_from_sadp(&reply) {
                discovered.push(cam);
            }
        }
    }

    if total_datagrams_received == 0 {
        return DiscoveryOutcome::MulticastSilence;
    }

    DiscoveryOutcome::Discovered(dedupe(discovered))
}

fn dedupe(found: Vec<DiscoveredCamera>) -> Vec<DiscoveredCamera> {
    let mut by_id: BTreeMap<String, DiscoveredCamera> = BTreeMap::new();
    for camera in found {
        by_id.entry(camera.id.clone()).or_insert(camera);
    }
    by_id.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeDatagram {
        wsd: Vec<Vec<u8>>,
        sadp: Vec<Vec<u8>>,
    }

    impl Datagram for FakeDatagram {
        fn exchange(&self, group: SocketAddr, _req: &[u8], _timeout: Duration) -> Vec<Vec<u8>> {
            if group.port() == WSD_PORT {
                self.wsd.clone()
            } else if group.port() == SADP_PORT {
                self.sadp.clone()
            } else {
                Vec::new()
            }
        }
    }

    const SADP_REPLY: &str = r#"<?xml version="1.0" encoding="utf-8"?><ProbeMatch>
        <Uuid>11112222-3333-4444-5555-666677778888</Uuid>
        <Types>inquiry</Types>
        <DeviceType>DS-2CD2042WD-I</DeviceType>
        <DeviceDescription>Camera</DeviceDescription>
        <IPv4Address>192.168.1.64</IPv4Address>
        <MAC>44-19-b6-aa-bb-cc</MAC>
        <SoftwareVersion>V5.5.82</SoftwareVersion>
    </ProbeMatch>"#;

    #[test]
    fn multicast_silence_when_zero_datagrams() {
        let fake = FakeDatagram {
            wsd: vec![],
            sadp: vec![],
        };
        let outcome = scan_multicast_with(&fake, Duration::from_millis(10));
        assert_eq!(outcome, DiscoveryOutcome::MulticastSilence);
    }

    #[test]
    fn multicast_discovers_sadp_and_wsd() {
        let wsd_doc = r#"<s:Envelope><s:Body><d:ProbeMatches><d:ProbeMatch>
            <w:EndpointReference><w:Address>urn:uuid:wsd-camera-1</w:Address></w:EndpointReference>
            <d:Types>tds:Device</d:Types>
            <d:Scopes>onvif://www.onvif.org/name/Cam1 onvif://www.onvif.org/hardware/ModelX</d:Scopes>
            <d:XAddrs>http://192.168.1.50/onvif/device_service</d:XAddrs>
            </d:ProbeMatch></d:ProbeMatches></s:Body></s:Envelope>"#;

        let fake = FakeDatagram {
            wsd: vec![wsd_doc.as_bytes().to_vec()],
            sadp: vec![SADP_REPLY.as_bytes().to_vec()],
        };

        let outcome = scan_multicast_with(&fake, Duration::from_millis(10));
        match outcome {
            DiscoveryOutcome::Discovered(cameras) => {
                assert_eq!(cameras.len(), 2);
                let sadp_cam = cameras
                    .iter()
                    .find(|c| c.probe.model == "DS-2CD2042WD-I")
                    .unwrap();
                assert_eq!(sadp_cam.probe.vendor, "Hikvision");
                assert_eq!(sadp_cam.probe.mac.as_deref(), Some("44-19-b6-aa-bb-cc"));
                assert_eq!(sadp_cam.probe.ip.as_deref(), Some("192.168.1.64"));
            }
            DiscoveryOutcome::MulticastSilence => panic!("Expected Discovered"),
        }
    }

    #[test]
    fn sadp_probe_format() {
        let probe = sadp_probe("test-uuid");
        assert_eq!(
            probe,
            "<Probe><Uuid>test-uuid</Uuid><Types>inquiry</Types></Probe>"
        );
    }
}
