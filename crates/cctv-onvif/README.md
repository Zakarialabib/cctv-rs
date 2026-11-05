# cctv-onvif

[![Crates.io](https://img.shields.io/crates/v/cctv-onvif.svg)](https://crates.io/crates/cctv-onvif)
[![docs.rs](https://docs.rs/cctv-onvif/badge.svg)](https://docs.rs/cctv-onvif)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)

ONVIF protocol driver implementation for `cctv-rs`. Provides Device, Media, and PTZ services over SOAP 1.2 with WS-Security `UsernameToken` authentication.

Maps ONVIF XML protocols cleanly into the standard `CameraBackend` trait.

## Primitives

- **WS-Security Auth**: Nonce, ISO 8601 UTC timestamp, and SHA-1 password digest calculation.
- **Clock-Skew Compensation**: Adjusts `<wsu:Created>` timestamp using observed camera clock offset so authentication succeeds even on drifted devices.
- **Device Management**: Queries device information, capabilities, and dynamic `XAddr` service endpoints.
- **Media Profiles**: Resolves RTSP stream URLs and JPEG snapshot endpoints.
- **PTZ Service**: Continuous movement, absolute positioning, velocity clamping, and preset storage.
- **Discovery Delegate**: `OnvifDiscovery` delegates sweeps to `cctv-discovery` while filtering for verified ONVIF XAddrs.

## Invariants

- `#![forbid(unsafe_code)]`
- Namespace-prefix-agnostic XML parsing.

## Usage

```rust,no_run
use cctv_onvif::{Credentials, OnvifSession};
use cctv_transport::http::HttpConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = OnvifSession::new(
        "http://192.168.1.64/onvif/device_service",
        Credentials::new("admin", "secret123"),
        HttpConfig::default(),
    );

    let profiles = session.get_profiles().await?;
    println!("Media profiles: {}", profiles.len());
    Ok(())
}
```
