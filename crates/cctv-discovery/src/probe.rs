//! The probe surface: one sweep, the transport it runs on, and the item it
//! yields.

use std::collections::BTreeMap;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cctv_core::ProbeResult;

use crate::{ssdp, wsd};

/// One camera found by a probe sweep.
///
/// `id` is the stable identity discovery observed — the SSDP `USN`, or the
/// WS-Discovery endpoint address — and is what the fleet adopts the camera
/// under.  `service_url` is the device's control-service endpoint when the
/// protocol advertised one (a WS-Discovery `XAddr`), which becomes
/// `CameraConfig::service_url` on adoption.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DiscoveredCamera {
    /// Stable identity discovery observed.
    pub id: String,
    /// The device-model probe fields for this camera.
    pub probe: ProbeResult,
    /// Control-service URL the device advertised, if any.
    pub service_url: Option<String>,
}

impl DiscoveredCamera {
    /// Build a discovered camera from an identity and its probe fields.
    pub fn new(id: impl Into<String>, probe: ProbeResult) -> Self {
        Self {
            id: id.into(),
            probe,
            service_url: None,
        }
    }
}

/// The datagram transport a sweep runs on.
///
/// Implemented for the real UDP socket ([`UdpTransport`]) and for test doubles,
/// so the sweep logic is exercised without a network.
pub trait Datagram {
    /// Send `request` to `group` and collect every datagram that arrives before
    /// `timeout` elapses.
    ///
    /// Must return whatever it has once the timeout passes rather than blocking
    /// indefinitely, and must never panic: an unreachable network yields an
    /// empty sweep, not an error.
    fn exchange(&self, group: SocketAddr, request: &[u8], timeout: Duration) -> Vec<Vec<u8>>;
}

/// A UDP transport over a real socket, one ephemeral port per exchange.
///
/// Replies to a multicast `M-SEARCH` / WS-Discovery `Probe` come back **unicast
/// to the source port**, so an ephemeral bind is enough — no multicast-group
/// membership is needed to hear the answers.
#[derive(Debug, Clone, Copy)]
pub struct UdpTransport {
    ttl: u32,
}

impl UdpTransport {
    /// A transport with the usual LAN multicast TTL (4).
    pub fn new() -> Self {
        Self { ttl: 4 }
    }

    /// Override the multicast TTL.
    pub fn with_ttl(mut self, ttl: u32) -> Self {
        self.ttl = ttl.max(1);
        self
    }
}

impl Default for UdpTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Datagram for UdpTransport {
    fn exchange(&self, group: SocketAddr, request: &[u8], timeout: Duration) -> Vec<Vec<u8>> {
        let Ok(socket) = UdpSocket::bind(("0.0.0.0", 0)) else {
            return Vec::new();
        };
        let _ = socket.set_multicast_ttl_v4(self.ttl);
        if socket.send_to(request, group).is_err() {
            return Vec::new();
        }
        let deadline = Instant::now() + timeout;
        let mut replies = Vec::new();
        let mut buf = vec![0u8; 65_536];
        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            if remaining.is_zero() || socket.set_read_timeout(Some(remaining)).is_err() {
                break;
            }
            match socket.recv_from(&mut buf) {
                Ok((n, _)) => replies.push(buf[..n].to_vec()),
                Err(_) => break,
            }
        }
        replies
    }
}

/// A discovery probe.  Stateless: each [`Probe::scan`] is one sweep.
#[derive(Debug, Clone, Copy)]
pub struct Probe {
    timeout: Duration,
    mx: u8,
}

impl Probe {
    /// A probe with a 1.5 s collection window and `MX: 2`.
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_millis(1_500),
            mx: 2,
        }
    }

    /// Override how long a sweep collects replies.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Override the SSDP `MX` — the maximum delay a device may answer after.
    pub fn with_mx(mut self, mx: u8) -> Self {
        self.mx = mx.max(1);
        self
    }

    /// Sweep the LAN over a real UDP socket.
    ///
    /// Both protocols are tried — SSDP `M-SEARCH` on UDP 1900 and a
    /// WS-Discovery `Probe` on UDP 3702 — and the results are merged, deduped
    /// by identity and ordered by it.  An unreachable network yields an empty
    /// sweep.
    pub fn scan(&self) -> Vec<DiscoveredCamera> {
        self.scan_with(&UdpTransport::new())
    }

    /// Sweep over an injected transport.
    ///
    /// The pure half of [`Probe::scan`], and the seam the tests use.
    pub fn scan_with(&self, net: &dyn Datagram) -> Vec<DiscoveredCamera> {
        let mut found = Vec::new();

        let ssdp_group = SocketAddr::from(([239, 255, 255, 250], ssdp::PORT));
        let request = ssdp::m_search(ssdp::SEARCH_ALL, self.mx);
        for datagram in net.exchange(ssdp_group, request.as_bytes(), self.timeout) {
            if let Some(reply) = ssdp::parse_reply(&datagram) {
                if let Some(camera) = camera_from_ssdp(&reply) {
                    found.push(camera);
                }
            }
        }

        let wsd_group = SocketAddr::from(([239, 255, 255, 250], wsd::PORT));
        let request = wsd::probe(&message_id());
        for datagram in net.exchange(wsd_group, request.as_bytes(), self.timeout) {
            let document = String::from_utf8_lossy(&datagram);
            for matched in wsd::parse_probe_matches(&document) {
                if let Some(camera) = camera_from_probe_match(&matched) {
                    found.push(camera);
                }
            }
        }

        dedupe(found)
    }
}

impl Default for Probe {
    fn default() -> Self {
        Self::new()
    }
}

/// A WS-Discovery `MessageID`: fresh per sweep and shaped like a UUID v4.
///
/// The id only has to be unique per sweep, so the clock, the process id and a
/// counter are enough — it is not a cryptographic value.
fn message_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);

    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&nanos.to_be_bytes());
    bytes[8..12].copy_from_slice(&std::process::id().to_be_bytes());
    bytes[12..].copy_from_slice(&(sequence as u32).to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant

    let hex = |range: std::ops::Range<usize>| {
        bytes[range]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        hex(0..4),
        hex(4..6),
        hex(6..8),
        hex(8..10),
        hex(10..16)
    )
}

/// Turn an SSDP reply into a discovered camera.
fn camera_from_ssdp(reply: &ssdp::SsdpReply) -> Option<DiscoveredCamera> {
    let id = reply
        .usn()
        .map(str::to_owned)
        .or_else(|| reply.location().map(str::to_owned))
        .or_else(|| reply.host())?;
    let (vendor, model) = reply
        .server()
        .map(ssdp::server_vendor_model)
        .unwrap_or_default();
    Some(DiscoveredCamera {
        id,
        probe: ProbeResult {
            vendor,
            model,
            firmware: None,
            ip: reply.host(),
            mac: None,
            stream_url: None,
            ptz_supported: false,
            ai_supported: false,
        },
        service_url: None,
    })
}

/// Turn a WS-Discovery `ProbeMatch` into a discovered camera.
fn camera_from_probe_match(matched: &wsd::ProbeMatch) -> Option<DiscoveredCamera> {
    let service_url = matched.service_url().map(str::to_owned);
    let id = if matched.endpoint.is_empty() {
        service_url.clone()?
    } else {
        matched.endpoint.clone()
    };
    let ip = service_url.as_deref().and_then(ssdp::host_of);
    let vendor = matched
        .scope("manufacturer")
        .or_else(|| matched.scope("name"))
        .unwrap_or_default()
        .to_owned();
    let model = matched
        .scope("model")
        .or_else(|| matched.scope("hardware"))
        .unwrap_or_default()
        .to_owned();
    Some(DiscoveredCamera {
        id,
        probe: ProbeResult {
            vendor,
            model,
            firmware: None,
            ip,
            mac: None,
            stream_url: None,
            ptz_supported: false,
            ai_supported: false,
        },
        service_url,
    })
}

/// Keep the first sighting of each identity, ordered by identity.
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

    /// A transport that answers with canned datagrams, chosen by port.
    struct Fake {
        ssdp: Vec<Vec<u8>>,
        wsd: Vec<Vec<u8>>,
    }

    impl Fake {
        fn new(ssdp: &[&str], wsd: &[&str]) -> Self {
            Self {
                ssdp: ssdp.iter().map(|s| s.as_bytes().to_vec()).collect(),
                wsd: wsd.iter().map(|s| s.as_bytes().to_vec()).collect(),
            }
        }
    }

    impl Datagram for Fake {
        fn exchange(&self, group: SocketAddr, _request: &[u8], _timeout: Duration) -> Vec<Vec<u8>> {
            if group.port() == ssdp::PORT {
                self.ssdp.clone()
            } else if group.port() == wsd::PORT {
                self.wsd.clone()
            } else {
                Vec::new()
            }
        }
    }

    const SSDP_REPLY: &str = "HTTP/1.1 200 OK\r\n\
        LOCATION: http://192.168.1.64:80/onvif/device_service\r\n\
        SERVER: Linux/3.14 UPnP/1.0 IpCamera/1.0 HIKVISION/5.50\r\n\
        USN: uuid:aaaa-1111\r\n\r\n";

    const WSD_REPLY: &str = r#"<s:Envelope><s:Body><d:ProbeMatches><d:ProbeMatch>
        <w:EndpointReference><w:Address>urn:uuid:bbbb-2222</w:Address></w:EndpointReference>
        <d:Types>tds:Device</d:Types>
        <d:Scopes>onvif://www.onvif.org/name/Front%20Door onvif://www.onvif.org/hardware/DS-2CD2143G0</d:Scopes>
        <d:XAddrs>http://192.168.1.65/onvif/device_service</d:XAddrs>
        </d:ProbeMatch></d:ProbeMatches></s:Body></s:Envelope>"#;

    #[test]
    fn a_sweep_merges_both_protocols_and_orders_by_identity() {
        let net = Fake::new(&[SSDP_REPLY], &[WSD_REPLY]);
        let found = Probe::new().scan_with(&net);
        assert_eq!(found.len(), 2);

        let ssdp = found.iter().find(|c| c.id == "uuid:aaaa-1111").unwrap();
        assert_eq!(ssdp.probe.vendor, "IpCamera");
        assert_eq!(ssdp.probe.model, "HIKVISION");
        assert_eq!(ssdp.probe.ip.as_deref(), Some("192.168.1.64"));
        assert_eq!(ssdp.service_url, None);

        let wsd = found.iter().find(|c| c.id == "urn:uuid:bbbb-2222").unwrap();
        assert_eq!(wsd.probe.vendor, "Front%20Door");
        assert_eq!(wsd.probe.model, "DS-2CD2143G0");
        assert_eq!(wsd.probe.ip.as_deref(), Some("192.168.1.65"));
        assert_eq!(
            wsd.service_url.as_deref(),
            Some("http://192.168.1.65/onvif/device_service")
        );

        let mut ids: Vec<&str> = found.iter().map(|c| c.id.as_str()).collect();
        let sorted = ids.clone();
        ids.sort_unstable();
        assert_eq!(ids, sorted, "the sweep is ordered by identity");
    }

    #[test]
    fn one_identity_reported_twice_is_reported_once() {
        let ssdp = SSDP_REPLY.replace("uuid:aaaa-1111", "urn:uuid:bbbb-2222");
        let net = Fake::new(&[&ssdp], &[WSD_REPLY]);
        let found = Probe::new().scan_with(&net);
        assert_eq!(found.len(), 1, "the identity is deduped");
        assert_eq!(found[0].id, "urn:uuid:bbbb-2222");
    }

    #[test]
    fn garbage_and_silence_yield_an_empty_sweep() {
        let net = Fake::new(&["not ssdp at all", "\x00\x01\x02"], &["<html>nope</html>"]);
        assert!(Probe::new().scan_with(&net).is_empty());
        let silent = Fake::new(&[], &[]);
        assert!(Probe::new().scan_with(&silent).is_empty());
    }

    #[test]
    fn a_probe_match_with_no_endpoint_falls_back_to_its_service_url() {
        let reply = r#"<d:ProbeMatches><d:ProbeMatch>
            <d:XAddrs>http://10.0.0.9/onvif/device_service</d:XAddrs>
            </d:ProbeMatch></d:ProbeMatches>"#;
        let net = Fake::new(&[], &[reply]);
        let found = Probe::new().scan_with(&net);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "http://10.0.0.9/onvif/device_service");
    }

    #[test]
    fn message_ids_are_unique_and_uuid_shaped() {
        let first = message_id();
        let second = message_id();
        assert_ne!(first, second);
        assert!(first.starts_with("urn:uuid:"));
        let body = first.trim_start_matches("urn:uuid:");
        let groups: Vec<usize> = body.split('-').map(str::len).collect();
        assert_eq!(groups, vec![8, 4, 4, 4, 12]);
        assert!(body.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
        assert_eq!(body.as_bytes()[14], b'4', "version nibble");
    }

    #[test]
    fn the_probe_defaults_are_usable() {
        let probe = Probe::default()
            .with_timeout(Duration::from_millis(5))
            .with_mx(0);
        assert_eq!(probe.mx, 1, "MX is clamped to at least 1");
        assert_eq!(probe.timeout, Duration::from_millis(5));
        assert_eq!(UdpTransport::default().ttl, 4);
        assert_eq!(UdpTransport::new().with_ttl(0).ttl, 1);
    }
}
