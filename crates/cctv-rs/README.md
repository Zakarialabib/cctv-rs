# cctv-rs

[![Crates.io](https://img.shields.io/crates/v/cctv-rs.svg)](https://crates.io/crates/cctv-rs)
[![docs.rs](https://docs.rs/cctv-rs/badge.svg)](https://docs.rs/cctv-rs)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Unified aggregate facade for the `cctv-rs` workspace. Exposes camera discovery, streaming, PTZ control, detection zones, and fleet reconciliation behind a single, ergonomic entry point.

100% vendor-agnostic public API relying exclusively on `cctv-core` device types.

## Facade Capabilities

- **Registry Management**: `attach`, `register`, `unregister`, `list_cameras`, and `config`.
- **Command Dispatch**: Universal `execute(camera_id, command)` entry point accepting any `CommandId`.
- **Media Controls**: `get_camera_frame`, `start_camera_stream`, `ptz_move`, and preset operations (`save`, `goto`, `delete`).
- **Zone & Detection Surface**: `zone_save`, `zone_list`, `zone_config`, `zone_delete`, and `record_detection` motion-event gating.
- **Network Sweeps**: Synchronous multi-protocol network sweeps (`discover`, `discover_with`).
- **Fleet Reconciler**: Reconciles observed against desired fleet state, returning audited remediation steps.
- **Extensible Drivers**: Dynamic `DriverFactory` registry mapping driver schemes to backend instances (`with_builtin_drivers` pre-wires ONVIF and Ezviz).

## Invariants

- `#![forbid(unsafe_code)]`
- Zero vendor-specific leakages across public types and method signatures.

## Usage

```rust,no_run
use cctv_rs::{CameraConfig, CommandId, Facade};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut facade = Facade::new().with_builtin_drivers();

    let config = CameraConfig::new("cam-01", "onvif", "192.168.1.100");
    let handle = facade.attach(config)?;

    let frame = facade.execute(handle.id(), CommandId::GetCameraFrame).await?;
    println!("Frame metadata: {:?}", frame);

    Ok(())
}
```
