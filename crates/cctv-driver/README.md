# cctv-driver

[![Crates.io](https://img.shields.io/crates/v/cctv-driver.svg)](https://crates.io/crates/cctv-driver)
[![docs.rs](https://docs.rs/cctv-driver/badge.svg)](https://docs.rs/cctv-driver)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)

Vendor-neutral driver extension point for `cctv-rs`. Separates driver factories
(`DriverAdapter`) from live connected camera handles (`CameraBackend`), and ships
the bundled protocol adapters behind cargo features.

The seam itself depends only on `cctv-core`; every adapter is opt-in, so a host
that speaks one protocol compiles none of the others.

## Features

| Feature | Default | Pulls in |
| --- | --- | --- |
| `discovery` | yes | SSDP (`M-SEARCH`) and WS-Discovery (`Probe`) UDP sweeps over `cctv-transport`. |
| `driver-mock` | no | `MockDriverAdapter` fixture for host-side and conformance testing. |
| `mobile` | no | `discovery` plus the `tokio` runtime for on-device sweeps. |
| `ezviz` | no | Ezviz Open Platform adapter wrapping `ezviz-openapi`. |
| `pc-sdk` | no | `ezviz` plus the vendor PC-SDK backend (`ezviz-openapi/pc-sdk`). |
| `uvc` | no | Local video seam: mockable OS platform, UVC capture, budgeted frame reading, HAL hot-plug events. |
| `uvc-ptp` | no | `uvc` plus Picture-Transfer-Protocol fetch surfaces. |
| `ddc` | no | `uvc` plus DDC/CI display control (VCP codes, input source, power mode). |

## Primitives

- **Adapter Seam**: `DriverAdapter` (factory: `driver_key`, `capabilities`, `probe`, `attach`) and the `CameraBackend` live handle.
- **Registry**: `DriverRegistry` / `InMemoryRegistry` mapping a driver key to a factory, with typed `DriverError`.
- **Discovery**: `Probe` with `Datagram`-injected sweeps, returning `DiscoveredCamera`, `Discoverable`.
- **Ezviz** (`ezviz`): `EzvizDriverAdapter`, `EzvizCameraBackend`, typed `EzvizError`.
- **Local Video** (`uvc`): `UvcAdapter` / `UvcDriverAdapter`, `BudgetedFrameReader`, `OsPlatform` seam, `HalEventSource`.
- **PTP** (`uvc-ptp`): `PtpAdapter` / `PtpDriverAdapter`, `PtpStatus`.
- **DDC/CI** (`ddc`): `DdcController`, `I2cBus` (with `MockI2cBus`), `VcpCode`, `InputSource`, `PowerMode`.

## Invariants

- `#![forbid(unsafe_code)]`
- The seam depends only on `cctv-core`; `cctv-transport` is linked only under `discovery`.
- Optional adapters never leak into the facade's public surface as vendor names.

## Usage

```rust,no_run
use std::sync::Arc;

use cctv_core::CameraConfig;
use cctv_driver::{DriverRegistry, InMemoryRegistry};

// With the `driver-mock` feature enabled.
use cctv_driver::MockDriverAdapter;

# async fn run() -> Result<(), cctv_core::CameraError> {
let mut registry = InMemoryRegistry::new();
registry.register(Arc::new(MockDriverAdapter::new("mock")));
assert_eq!(registry.keys(), vec!["mock".to_string()]);

// Route an attach through the registry: driver key -> factory -> live handle.
let config = CameraConfig::new("cam-01", "mock", "127.0.0.1");
let backend = registry.attach(&config).await?;
let _ = backend;
# Ok(())
# }
```

## License & Security

- **License:** Apache-2.0 ([LICENSE](../../LICENSE))
- **Security:** Vulnerability disclosure process ([SECURITY.md](../../SECURITY.md))
