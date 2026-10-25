//! SSDP (Simple Service Discovery Protocol) message codec.
//!
//! Pure: the `M-SEARCH` request is built and the replies are parsed here; the
//! UDP exchange lives in [`crate::probe`].  A reply is either an `M-SEARCH`
//! response (`HTTP/1.1 200 OK`) or a `NOTIFY` advertisement — both a start line
//! followed by `Name: value` headers.
//!
//! UPnP is the baseline every IP camera speaks, so this is the widest net.  The
//! `SERVER` header is the only place a vendor or model appears and is read as a
//! *heuristic* ([`server_vendor_model`]), never as a contract.

/// The SSDP multicast group.
pub const MULTICAST_ADDR: &str = "239.255.255.250";
/// The SSDP port.
pub const PORT: u16 = 1900;
/// Search for every device rather than one device type.
pub const SEARCH_ALL: &str = "ssdp:all";

/// Build an `M-SEARCH` datagram.
///
/// `search_target` is the `ST` (`ssdp:all` for everything); `mx` is the maximum
/// number of seconds a device may delay its answer by.
pub fn m_search(search_target: &str, mx: u8) -> String {
    format!(
        "M-SEARCH * HTTP/1.1\r\n\
         HOST: {MULTICAST_ADDR}:{PORT}\r\n\
         MAN: \"ssdp:discover\"\r\n\
         MX: {mx}\r\n\
         ST: {search_target}\r\n\r\n"
    )
}

/// One parsed SSDP reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsdpReply {
    /// The start line: `HTTP/1.1 200 OK` or `NOTIFY * HTTP/1.1`.
    pub start_line: String,
    /// Header name/value pairs, names lower-cased.
    pub headers: Vec<(String, String)>,
}

impl SsdpReply {
    /// First header with this (case-insensitive) name.
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
    }

    /// `true` when the start line is an `M-SEARCH` response.
    pub fn is_response(&self) -> bool {
        let mut words = self.start_line.split_whitespace();
        words.next() == Some("HTTP/1.1") && words.next() == Some("200")
    }

    /// The advertised `LOCATION` URL of the device description.
    pub fn location(&self) -> Option<&str> {
        self.header("location")
    }

    /// The `SERVER` header (OS / UPnP / product tokens).
    pub fn server(&self) -> Option<&str> {
        self.header("server")
    }

    /// The `USN` — the device's stable unique service name.
    pub fn usn(&self) -> Option<&str> {
        self.header("usn")
    }

    /// The `ST` — the search target / device type.
    pub fn st(&self) -> Option<&str> {
        self.header("st")
    }

    /// The host of [`SsdpReply::location`], when it parses.
    pub fn host(&self) -> Option<String> {
        host_of(self.location()?)
    }
}

/// Parse one SSDP datagram.
///
/// `None` for anything that is not a header block; never panics on a truncated
/// or binary datagram.
pub fn parse_reply(datagram: &[u8]) -> Option<SsdpReply> {
    let text = std::str::from_utf8(datagram).ok()?;
    let mut lines = text.split("\r\n");
    let start_line = lines.next()?.trim().to_string();
    if !start_line.starts_with("HTTP/1.") && !start_line.starts_with("NOTIFY") {
        return None;
    }
    let headers = lines
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    Some(SsdpReply {
        start_line,
        headers,
    })
}

/// The host of a `scheme://host[:port]/...` URL, or of a bare `host:port`.
///
/// An IPv6 literal in brackets loses its brackets; an empty host is `None`.
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = authority.rsplit('@').next().unwrap_or(authority);
    if let Some(bracketed) = host.strip_prefix('[') {
        return bracketed.split_once(']').map(|(v6, _)| v6.to_string());
    }
    let host = host.split(':').next().unwrap_or(host);
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

/// Split a `SERVER` header into `(vendor, model)`.
///
/// `SERVER` looks like `Linux/3.14 UPnP/1.0 IpCamera/1.0 AXIS-221/5.50`: the OS
/// and `UPnP/` tokens are dropped, the first remaining product becomes the
/// vendor and the last the model.  A device that reports a single product
/// reports it as both.
pub fn server_vendor_model(server: &str) -> (String, String) {
    let products: Vec<&str> = server
        .split_whitespace()
        .filter(|token| !is_stack_token(token))
        .collect();
    let name = |token: &str| token.split('/').next().unwrap_or(token).to_string();
    match products.as_slice() {
        [] => (String::new(), String::new()),
        [only] => (name(only), name(only)),
        [first, .., last] => (name(first), name(last)),
    }
}

/// `true` for the OS / UPnP tokens a `SERVER` header carries.
fn is_stack_token(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    ["upnp/", "linux", "windows", "darwin", "freebsd", "android"]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESPONSE: &str = "HTTP/1.1 200 OK\r\n\
        CACHE-CONTROL: max-age=1800\r\n\
        LOCATION: http://192.168.1.64:80/onvif/device_service\r\n\
        SERVER: Linux/3.14 UPnP/1.0 IpCamera/1.0 HIKVISION/5.50\r\n\
        ST: urn:schemas-upnp-org:device:Basic:1\r\n\
        USN: uuid:3fa1fe68-b915-4053-a3e1-1000b1a2b3c4::urn:schemas-upnp-org:device:Basic:1\r\n\
        \r\n";

    #[test]
    fn m_search_is_a_well_formed_discovery_request() {
        let request = m_search(SEARCH_ALL, 2);
        assert!(request.starts_with("M-SEARCH * HTTP/1.1\r\n"));
        assert!(request.contains("HOST: 239.255.255.250:1900\r\n"));
        assert!(request.contains("MAN: \"ssdp:discover\"\r\n"));
        assert!(request.contains("MX: 2\r\n"));
        assert!(request.contains("ST: ssdp:all\r\n"));
        assert!(request.ends_with("\r\n\r\n"), "terminated by a blank line");
    }

    #[test]
    fn a_200_response_is_read_header_by_header() {
        let reply = parse_reply(RESPONSE.as_bytes()).expect("a response");
        assert!(reply.is_response());
        assert_eq!(reply.st(), Some("urn:schemas-upnp-org:device:Basic:1"));
        assert!(reply.usn().unwrap().starts_with("uuid:3fa1fe68"));
        assert_eq!(
            reply.location(),
            Some("http://192.168.1.64:80/onvif/device_service")
        );
        assert_eq!(reply.host().as_deref(), Some("192.168.1.64"));
        assert_eq!(reply.header("CACHE-CONTROL"), Some("max-age=1800"));
    }

    #[test]
    fn a_notify_advertisement_is_parsed_but_is_not_a_response() {
        let notify = "NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\n\
            NT: urn:schemas-upnp-org:device:Basic:1\r\nUSN: uuid:abc\r\n\r\n";
        let reply = parse_reply(notify.as_bytes()).expect("an advertisement");
        assert!(!reply.is_response());
        assert_eq!(reply.usn(), Some("uuid:abc"));
        assert_eq!(reply.location(), None);
        assert_eq!(reply.host(), None);
    }

    #[test]
    fn garbage_is_not_a_reply() {
        assert_eq!(parse_reply(b""), None);
        assert_eq!(parse_reply(b"\x00\x01\x02\x03"), None);
        assert_eq!(parse_reply(b"GET / HTTP/1.1\r\n\r\n"), None);
        // A header block with no start line is not a reply either.
        assert_eq!(parse_reply(b"LOCATION: http://x/\r\n\r\n"), None);
    }

    #[test]
    fn host_of_reads_the_authority_only() {
        assert_eq!(
            host_of("http://10.0.0.5:8080/a?b=1#c").as_deref(),
            Some("10.0.0.5")
        );
        assert_eq!(
            host_of("rtsp://admin:pw@cam.local:554/s").as_deref(),
            Some("cam.local")
        );
        assert_eq!(host_of("http://[fe80::1]:80/d").as_deref(), Some("fe80::1"));
        assert_eq!(host_of("http:///nohost"), None);
        assert_eq!(host_of("bare-host:1900").as_deref(), Some("bare-host"));
    }

    #[test]
    fn server_vendor_model_drops_the_stack_tokens() {
        assert_eq!(
            server_vendor_model("Linux/3.14 UPnP/1.0 IpCamera/1.0 AXIS-221/5.50"),
            ("IpCamera".to_string(), "AXIS-221".to_string())
        );
        assert_eq!(
            server_vendor_model("Windows/10 UPnP/1.0 Hikvision-Webs/1.0"),
            ("Hikvision-Webs".to_string(), "Hikvision-Webs".to_string())
        );
        assert_eq!(
            server_vendor_model("UPnP/1.0"),
            (String::new(), String::new())
        );
        assert_eq!(server_vendor_model(""), (String::new(), String::new()));
    }
}
