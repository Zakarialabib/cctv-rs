# cctv-discovery

[![Crates.io](https://img.shields.io/crates/v/cctv-discovery.svg)](https://crates.io/crates/cctv-discovery)
[![docs.rs](https://docs.rs/cctv-discovery/badge.svg)](https://docs.rs/cctv-discovery)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)

Stateless, zero-configuration IP camera discovery over UDP multicast sweeps.

Executes dual SSDP `M-SEARCH` and WS-Discovery `Probe` sweeps, deduplicating devices by endpoint and hardware identity.

## Primitives

- **SSDP**: Multicast sweeps targeting `239.255.255.250:1900` for UPnP / RTSP camera profiles.
- **WS-Discovery**: SOAP `Probe` datagrams over `239.255.255.250:3702` for ONVIF camera endpoints.
- **Deduplication Engine**: Merges multi-protocol responses into deterministic `DiscoveredCamera` records.
- **Clock Probe**: Measures camera-to-host clock skew from network probe datagrams.

## Invariants

- `#![forbid(unsafe_code)]`
- Read-only network discovery surface; introduces no device state.

## Usage

```rust,no_run
use cctv_discovery::Probe;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let probe = Probe::default();
    let cameras = probe.scan().await?;

    for cam in cameras {
        println!("Found: {:?} at {}", cam.identity, cam.endpoint);
    }
    Ok(())
}
```
