//! Bounded mobile sweep across common CCTV camera ports.
//!
//! Uses a [`tokio::sync::Semaphore`] of 32 concurrent probes and a 250ms timeout across
//! ports:
//! - 80 (ONVIF)
//! - 554 (RTSP)
//! - 8000 (Hikvision)
//! - 37777 (Dahua)
//! - 10080 (Ezviz)
//!
//! When port 80 opens, runs `probe_onvif_clock`.
//! When port 8000 opens (Hikvision) and multicast was blocked, sends a unicast UDP
//! SADP XML probe directly to `<ip>:37020` to harvest the MAC address and model
//! without triggering HTTP 401 Unauthorized.

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::time::timeout;

use cctv_core::ProbeResult;

use crate::clock_probe::probe_onvif_clock;
use crate::multicast::{parse_sadp_reply, sadp_probe, SADP_PORT};
use crate::probe::{message_id, DiscoveredCamera};

/// The set of ports probed during mobile sweep.
pub const SWEEP_PORTS: [u16; 5] = [80, 554, 8000, 37777, 10080];

/// Maximum concurrency for mobile sweep tasks.
pub const MAX_CONCURRENCY: usize = 32;

/// Connect timeout per port probe.
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(250);

/// Result of probing a single IP address during a mobile sweep.
#[derive(Debug, Clone)]
pub struct HostProbeResult {
    /// The probed IP address.
    pub ip: IpAddr,
    /// Open ports detected.
    pub open_ports: Vec<u16>,
    /// Observed clock offset in milliseconds (if ONVIF port 80 opened and probe succeeded).
    pub clock_offset_ms: Option<i64>,
    /// Discovered camera information (if identified).
    pub camera: Option<DiscoveredCamera>,
}

/// Send a unicast UDP SADP XML inquiry probe directly to `<ip>:37020`.
pub fn probe_unicast_sadp(ip: IpAddr, timeout_dur: Duration) -> Option<DiscoveredCamera> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    let _ = socket.set_read_timeout(Some(timeout_dur));
    let target = SocketAddr::new(ip, SADP_PORT);
    let req = sadp_probe(&message_id());
    socket.send_to(req.as_bytes(), target).ok()?;

    let mut buf = vec![0u8; 4096];
    let (n, _) = socket.recv_from(&mut buf).ok()?;
    let reply_str = String::from_utf8_lossy(&buf[..n]);
    let parsed = parse_sadp_reply(&reply_str)?;

    let id = parsed
        .uuid
        .clone()
        .or_else(|| parsed.mac.clone())
        .unwrap_or_else(|| ip.to_string());

    Some(DiscoveredCamera {
        id,
        probe: ProbeResult {
            vendor: "Hikvision".to_string(),
            model: parsed.model.unwrap_or_default(),
            firmware: parsed.firmware,
            ip: Some(ip.to_string()),
            mac: parsed.mac,
            stream_url: None,
            ptz_supported: false,
            ai_supported: false,
        },
        service_url: Some(format!("http://{ip}/onvif/device_service")),
    })
}

/// Probe a single IP address across the standard camera ports.
pub async fn sweep_host(ip: IpAddr, multicast_blocked: bool, host_utc_ms: i64) -> HostProbeResult {
    let mut open_ports = Vec::new();

    for &port in &SWEEP_PORTS {
        let addr = SocketAddr::new(ip, port);
        if let Ok(Ok(_stream)) = timeout(PROBE_TIMEOUT, TcpStream::connect(addr)).await {
            open_ports.push(port);
        }
    }

    let mut clock_offset_ms = None;
    let mut camera = None;

    if open_ports.contains(&80) {
        if let Ok(offset) = probe_onvif_clock(ip, 80, host_utc_ms).await {
            clock_offset_ms = Some(offset);
        }
    }

    if open_ports.contains(&8000) && multicast_blocked {
        if let Some(sadp_cam) = probe_unicast_sadp(ip, PROBE_TIMEOUT) {
            camera = Some(sadp_cam);
        }
    }

    // If camera wasn't populated from SADP but port 80 or 554 opened, build generic DiscoveredCamera
    if camera.is_none() && !open_ports.is_empty() {
        let vendor = if open_ports.contains(&8000) {
            "Hikvision".to_string()
        } else if open_ports.contains(&37777) {
            "Dahua".to_string()
        } else if open_ports.contains(&10080) {
            "Ezviz".to_string()
        } else {
            "Generic".to_string()
        };

        camera = Some(DiscoveredCamera {
            id: ip.to_string(),
            probe: ProbeResult {
                vendor,
                model: String::new(),
                firmware: None,
                ip: Some(ip.to_string()),
                mac: None,
                stream_url: open_ports
                    .contains(&554)
                    .then(|| format!("rtsp://{ip}:554/live")),
                ptz_supported: false,
                ai_supported: false,
            },
            service_url: open_ports
                .contains(&80)
                .then(|| format!("http://{ip}/onvif/device_service")),
        });
    }

    HostProbeResult {
        ip,
        open_ports,
        clock_offset_ms,
        camera,
    }
}

/// Execute a bounded mobile sweep across a list of target IP addresses.
///
/// Bounded by a semaphore with 32 concurrent probes and 250ms per-port timeout.
pub async fn sweep_ips(
    ips: Vec<IpAddr>,
    multicast_blocked: bool,
    host_utc_ms: i64,
) -> Vec<HostProbeResult> {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENCY));
    let mut handles = Vec::with_capacity(ips.len());

    for ip in ips {
        let sem = Arc::clone(&semaphore);
        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.ok();
            sweep_host(ip, multicast_blocked, host_utc_ms).await
        });
        handles.push(handle);
    }

    let mut results = Vec::new();
    for handle in handles {
        if let Ok(res) = handle.await {
            if !res.open_ports.is_empty() {
                results.push(res);
            }
        }
    }

    results
}

/// Deduped cameras extracted from a mobile sweep.
pub fn cameras_from_sweep(results: &[HostProbeResult]) -> Vec<DiscoveredCamera> {
    let mut by_id: BTreeMap<String, DiscoveredCamera> = BTreeMap::new();
    for res in results {
        if let Some(cam) = &res.camera {
            by_id.entry(cam.id.clone()).or_insert_with(|| cam.clone());
        }
    }
    by_id.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, TcpListener};

    #[tokio::test]
    async fn sweep_detects_open_ports() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let addr = SocketAddr::new(ip, port);
        let res = timeout(PROBE_TIMEOUT, TcpStream::connect(addr)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn sweep_concurrency_semaphore_bounded() {
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENCY));
        assert_eq!(semaphore.available_permits(), 32);
    }

    #[tokio::test]
    async fn sweep_host_and_cameras_from_sweep() {
        let results = vec![HostProbeResult {
            ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)),
            open_ports: vec![80, 554],
            clock_offset_ms: Some(150),
            camera: Some(DiscoveredCamera {
                id: "cam-100".to_string(),
                probe: ProbeResult {
                    vendor: "Generic".to_string(),
                    model: "M1".to_string(),
                    firmware: None,
                    ip: Some("192.168.1.100".to_string()),
                    mac: None,
                    stream_url: Some("rtsp://192.168.1.100:554/live".to_string()),
                    ptz_supported: false,
                    ai_supported: false,
                },
                service_url: Some("http://192.168.1.100/onvif/device_service".to_string()),
            }),
        }];

        let cams = cameras_from_sweep(&results);
        assert_eq!(cams.len(), 1);
        assert_eq!(cams[0].id, "cam-100");
    }

    #[tokio::test]
    async fn unicast_sadp_probe_against_mock_udp() {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();

        let handle = std::thread::spawn(move || {
            let mut buf = [0u8; 1024];
            if let Ok((n, src)) = socket.recv_from(&mut buf) {
                let req = String::from_utf8_lossy(&buf[..n]);
                assert!(req.contains("<Probe>"));
                let reply = r#"<ProbeMatch>
                    <DeviceType>DS-2CD2042WD-I</DeviceType>
                    <IPv4Address>127.0.0.1</IPv4Address>
                    <MAC>aa-bb-cc-dd-ee-ff</MAC>
                </ProbeMatch>"#;
                let _ = socket.send_to(reply.as_bytes(), src);
            }
        });

        let client_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let req = sadp_probe("test-uuid");
        client_sock
            .send_to(
                req.as_bytes(),
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port),
            )
            .unwrap();

        let mut buf = [0u8; 1024];
        client_sock
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let (n, _) = client_sock.recv_from(&mut buf).unwrap();
        let reply_str = String::from_utf8_lossy(&buf[..n]);
        let parsed = parse_sadp_reply(&reply_str).expect("parse sadp reply");
        assert_eq!(parsed.model.as_deref(), Some("DS-2CD2042WD-I"));
        assert_eq!(parsed.mac.as_deref(), Some("aa-bb-cc-dd-ee-ff"));

        handle.join().unwrap();
    }
}
