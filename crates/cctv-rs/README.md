# cctv-rs

[![Crates.io](https://img.shields.io/crates/v/cctv-rs.svg)](https://crates.io/crates/cctv-rs)
[![docs.rs](https://docs.rs/cctv-rs/badge.svg)](https://docs.rs/cctv-rs)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

The unified aggregate facade for the `cctv-rs` workspace. Exposes camera discovery, streaming, PTZ control, and fleet reconciliation behind a single, clean API handle.

Public API surface is 100% vendor-agnostic and relies entirely on `cctv-core` device types.

## Facade API Surface

- **`Facade` Handle:**
  - **Registry Operations:** `register`, `attach`, `unregister`, `list_cameras`, `config`.
  - **Camera Controls:** `get_camera_frame`, `start_camera_stream`, `ptz_move`, `camera_config_get` / `set`, `camera_set_enabled`, preset operations (`save`, `goto`, `delete`).
  - **Command Execution:** Universal `execute(camera_id, command)` entry point accepting any `CommandId`.
  - **Discovery:** Async LAN sweeps (`discover`, `discover_with`).
  - **Fleet Management:** Fleet health checks and state reconciliation (`reconcile`).
- **`DriverFactory`:** Dynamic driver factory mapping configuration keys to backend instances (`Facade::with_builtin_drivers()` registers ONVIF and Ezviz).

## Usage Example

```rust,no_run
use cctv_rs::Facade;
use cctv_core::{CameraConfig, CameraHandle};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let facade = Facade::with_builtin_drivers();

    let handle = CameraHandle::from("camera-main");
    let config = CameraConfig::new("camera-main", "192.168.1.100", "onvif");
    facade.register(handle, config).await?;

    let frame = facade.get_camera_frame("camera-main").await?;
    println!("Captured frame snapshot with size {}x{}", frame.width, frame.height);

    Ok(())
}
```

## Technical Guarantees

- `#![forbid(unsafe_code)]`
- Zero vendor leak in public traits or methods.
