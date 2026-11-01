# cctv-rs

[![Crates.io](https://img.shields.io/crates/v/cctv-rs.svg)](https://crates.io/crates/cctv-rs)
[![docs.rs](https://docs.rs/cctv-rs/badge.svg)](https://docs.rs/cctv-rs)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

The unified aggregate facade for the `cctv-rs` workspace. Exposes camera discovery, streaming, PTZ control, detection zones, and fleet reconciliation behind a single, clean API handle.

Public API surface is 100% vendor-agnostic and relies entirely on `cctv-core` device types.

## Facade API Surface

- **`Facade` Handle:**
  - **Registry Operations:** `register`, `attach`, `unregister`, `list_cameras`, `config`.
  - **Camera Controls:** `get_camera_frame`, `start_camera_stream`, `ptz_move`, `camera_config_get` / `set`, `camera_set_enabled`, preset operations (`save`, `goto`, `delete`).
  - **Command Execution:** Universal `execute(camera_id, command)` entry point accepting any `CommandId`.
  - **Zone & Detection:** `zone_save` / `zone_list` / `zone_config` / `zone_delete` for `ZoneConfig`, plus `record_detection` — threshold- and window-gated motion reporting, answered through `ZoneMotionListEvents` on `execute`.
  - **Discovery:** Synchronous LAN sweeps (`discover`, `discover_with` — SSDP + WS-Discovery).
  - **Fleet Management:** Fleet health checks and state reconciliation (`reconcile`), with graded remediation from the embedded `Reconciler`.
- **`DriverFactory`:** Dynamic driver factory mapping configuration keys to backend instances (`Facade::new().with_builtin_drivers()` registers the ONVIF and Ezviz factories).
- **Agent re-exports:** `Reconciler`, `Remediation`, `CommandRelay`, `Grade`, `Action`, `Audit` — plan v0.2 W5 active remediation comes along for the ride.

## Usage Example

```rust,no_run
use cctv_rs::{CameraConfig, CommandId, Facade};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Builtin ONVIF + Ezviz driver factories
    let mut facade = Facade::new().with_builtin_drivers();

    // The driver key resolves through the factory registry
    let config = CameraConfig::new("camera-main", "onvif", "192.168.1.100");
    let handle = facade.attach(config)?;

    // Universal command entry point
    let frame = facade.execute(handle.id(), CommandId::GetCameraFrame).await?;
    println!("{:?}", frame);

    Ok(())
}
```

## Technical Guarantees

- `#![forbid(unsafe_code)]`
- Zero vendor leak in public traits or methods.
