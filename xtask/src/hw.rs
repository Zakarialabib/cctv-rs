//! `cargo xtask hw-test`: sweep the LAN for whatever cameras answer.
//!
//! The honest counterpart to the hosted test suite, which never touches a
//! device. It runs the same SSDP `M-SEARCH` and WS-Discovery `Probe` sweep the
//! facade runs, prints what answered, and exits successfully when nothing does:
//! "no camera here" is a fact to record, not a build failure.

use cctv_rs::{DiscoveredCamera, Facade};

/// Runs the sweep.
///
/// # Errors
///
/// Returns a message when an argument is malformed. An empty sweep is not an
/// error — it is the answer.
pub fn run(args: &[String]) -> Result<(), String> {
    let mut json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            other => return Err(format!("unknown argument '{other}'")),
        }
    }

    let found = Facade::new().discover();

    // SSDP on a Windows host answers with the local UPnP host service on
    // loopback. That is not a camera, and reporting it would make the tool look
    // broken in a way that hides the one fact worth knowing: there is no camera
    // here.
    let loopback_count = found.iter().filter(|camera| is_loopback(camera)).count();
    let mut seen = std::collections::BTreeSet::new();
    let cameras: Vec<_> = found
        .into_iter()
        .filter(|camera| !is_loopback(camera))
        .filter(|camera| seen.insert(camera.id.clone()))
        .collect();

    if cameras.is_empty() {
        println!(
            "No cameras answered the sweep. Nothing was verified.\n\
             This is a result, not a failure: it means no camera was reachable from this host.\n\
             {loopback_count} responder(s) were seen on loopback and ignored."
        );
        return Ok(());
    }

    if json {
        println!("[\n{}\n]", rows_json(&cameras));
    } else {
        print_table(&cameras);
    }

    Ok(())
}

fn is_loopback(camera: &DiscoveredCamera) -> bool {
    match camera.probe.ip.as_deref() {
        Some(ip) => ip.starts_with("127.") || ip == "::1" || ip == "localhost",
        None => false,
    }
}

fn print_table(cameras: &[DiscoveredCamera]) {
    println!(
        "{:<44} {:<12} {:<24} {:<16} SERVICE URL",
        "ID", "VENDOR", "MODEL", "ADDRESS"
    );
    for camera in cameras {
        println!(
            "{:<44} {:<12} {:<24} {:<16} {}",
            camera.id,
            camera.probe.vendor,
            camera.probe.model,
            camera.probe.ip.as_deref().unwrap_or("-"),
            camera.service_url.as_deref().unwrap_or("-"),
        );
    }
}

fn rows_json(cameras: &[DiscoveredCamera]) -> String {
    cameras
        .iter()
        .map(|camera| {
            format!(
                "{{\"id\":\"{}\",\"vendor\":\"{}\",\"model\":\"{}\",\"firmware\":\"{}\",\
                 \"address\":\"{}\",\"service_url\":\"{}\"}}",
                escape(&camera.id),
                escape(&camera.probe.vendor),
                escape(&camera.probe.model),
                escape(camera.probe.firmware.as_deref().unwrap_or("")),
                escape(camera.probe.ip.as_deref().unwrap_or("")),
                escape(camera.service_url.as_deref().unwrap_or("")),
            )
        })
        .collect::<Vec<_>>()
        .join(",\n")
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
