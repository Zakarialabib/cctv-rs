# cctv-ezviz

[![Crates.io](https://img.shields.io/crates/v/cctv-ezviz.svg)](https://crates.io/crates/cctv-ezviz)
[![docs.rs](https://docs.rs/cctv-ezviz/badge.svg)](https://docs.rs/cctv-ezviz)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Ezviz Open Platform cloud driver for `cctv-rs`. Wraps the `ezviz-openapi` client into `cctv-core` device model abstractions.

Converts Ezviz API endpoints into the uniform `CameraBackend` trait interface.

## Capabilities

- **Account Authentication & Session Tokens:** Dynamic token acquisition and region endpoint redirection.
- **Device & Channel Inventory:** Discovers account cameras, channels, and online connectivity state.
- **Stream URL Resolution:** Resolves dynamic RTSP / HLS playback stream URLs on demand.
- **PTZ & Presets:** Maps continuous PTZ direction and speed commands to platform actions; supports preset saving, goto, and deletion.
- **Typed Platform Gaps:** Explicitly exposes platform limitations (e.g. out-of-band capture) as typed `CameraError::Unsupported` variants.

## Usage Example

```rust,no_run
use cctv_ezviz::Driver;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let driver = Driver::new("app_key", "app_secret", "https://open.ezvizlife.com");
    // Connect and query device list
    Ok(())
}
```

## Technical Guarantees

- `#![forbid(unsafe_code)]`
- Tested against canned HTTP fixtures with simulated region redirection.
