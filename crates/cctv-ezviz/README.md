# cctv-ezviz

[![Crates.io](https://img.shields.io/crates/v/cctv-ezviz.svg)](https://crates.io/crates/cctv-ezviz)
[![docs.rs](https://docs.rs/cctv-ezviz/badge.svg)](https://docs.rs/cctv-ezviz)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)

Ezviz Open Platform cloud driver for `cctv-rs`. Wraps the `ezviz-openapi` client into standard `CameraBackend` abstractions.

## Primitives

- **Session Tokens**: Handles app key / secret authentication and regional token lifecycle.
- **Inventory Mapping**: Enumerates cloud-bound cameras, channels, and online connectivity state.
- **Stream URL Resolution**: Dynamically fetches temporary RTSP / HLS playback stream URLs.
- **PTZ & Presets**: Translates uniform PTZ directions, velocities, and preset calls to Ezviz cloud actions.
- **Fail-Closed Capabilities**: Unimplemented cloud functions fail closed with explicit `CameraError::Unsupported`.

## Invariants

- `#![forbid(unsafe_code)]`
- Implements `CameraBackend` without leaking platform-specific types.

## Usage

```rust,no_run
use cctv_ezviz::Driver;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _driver = Driver::new("app_key", "app_secret", "https://open.ezvizlife.com");
    Ok(())
}
```
