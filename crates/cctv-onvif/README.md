# cctv-onvif

[![Crates.io](https://img.shields.io/crates/v/cctv-onvif.svg)](https://crates.io/crates/cctv-onvif)
[![docs.rs](https://docs.rs/cctv-onvif/badge.svg)](https://docs.rs/cctv-onvif)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

ONVIF protocol driver implementation for `cctv-rs`. Provides Device, Media, and PTZ services over SOAP 1.2 with WS-Security `UsernameToken` authentication.

Translates ONVIF XML protocols cleanly into the standard `cctv-core` device model.

## Features

- **WS-Security Authentication:** Generates XML `UsernameToken` headers with `#PasswordDigest`, nonces, and ISO 8601 UTC timestamps.
- **Device Management:** `GetDeviceInformation`, `GetCapabilities`, endpoint caching across ONVIF `XAddr` service locations.
- **Media Management:** Profile listing, RTSP stream URI extraction, and JPEG snapshot URI acquisition.
- **PTZ Control:** Continuous PTZ movements, absolute positioning, stop commands, and preset management.
- **Namespace-Agnostic XML Parsing:** Tolerates non-standard vendor XML prefixes (`tds:`, `trt:`, `tptz:`, `SOAP-ENV:`).

## Usage Example

```rust,no_run
use cctv_onvif::{Credentials, OnvifSession};
use cctv_transport::http::HttpConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let session = OnvifSession::new(
        "http://192.168.1.64/onvif/device_service",
        Credentials { username: "admin".into(), password: "password".into() },
        HttpConfig::default(),
    )?;

    let dev_info = session.device_information().await?;
    println!("Device Model: {}", dev_info.model);

    let profiles = session.profiles().await?;
    println!("Found {} media profiles", profiles.len());

    Ok(())
}
```

## Technical Guarantees

- `#![forbid(unsafe_code)]`
- Conformance tested against mock SOAP fixtures.
