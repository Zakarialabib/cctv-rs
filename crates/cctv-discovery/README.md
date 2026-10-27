# cctv-discovery

[![Crates.io](https://img.shields.io/crates/v/cctv-discovery.svg)](https://crates.io/crates/cctv-discovery)
[![docs.rs](https://docs.rs/cctv-discovery/badge.svg)](https://docs.rs/cctv-discovery)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Zero-configuration IP camera discovery for `cctv-rs` via UDP multicast sweeps (SSDP `M-SEARCH` and WS-Discovery `Probe`).

Read-only, stateless network discovery surface returning strongly typed `DiscoveredCamera` structures.

## Discovery Protocols

- **SSDP (Simple Service Discovery Protocol):** UDP multicast search over `239.255.255.250:1900`.
- **WS-Discovery (Web Services Dynamic Discovery):** SOAP `Probe` envelopes over UDP multicast `239.255.255.250:3702`.
- **Deduplication & Merging:** Merges responses from both protocol sweeps, deduplicating devices by network identity and address.

## Usage Example

```rust,no_run
use cctv_discovery::Probe;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let probe = Probe::default();
    let cameras = probe.scan().await?;

    for cam in cameras {
        println!("Discovered IP Camera: {:?} at Endpoint: {}", cam.identity, cam.endpoint);
    }

    Ok(())
}
```

## Technical Guarantees

- `#![forbid(unsafe_code)]`
- Network errors return empty discovery sweeps rather than panicking or fabricating devices.
