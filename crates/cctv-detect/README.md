# cctv-detect

[![Crates.io](https://img.shields.io/crates/v/cctv-detect.svg)](https://crates.io/crates/cctv-detect)
[![docs.rs](https://docs.rs/cctv-detect/badge.svg)](https://docs.rs/cctv-detect)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)

Lightweight device model for motion and AI detection within `cctv-rs`.

Deliberately thin and runtime-free: no ML dependencies, no computer vision algorithms, and no I/O. The camera driver acquires the frame, host inference provides analysis, and `cctv-detect` models the resulting domain contracts.

## Primitives

- **`Detection`**: Individual detection event carrying motion or class classification, normalized confidence (`0.0..1.0`), and optional bounding geometry.
- **`Zone`**: Spatial boundaries evaluated for events: `point` (native coordinates) or `rectangle` (normalized `0.0..1.0`), clamped on instantiation.
- **`ZoneConfig`**: Threshold parameters: minimum confidence score, detection count threshold within window, and window duration in seconds.

## Invariants

- `#![forbid(unsafe_code)]`
- Total on inputs: out-of-range coordinates or thresholds are clamped rather than producing errors or panics.

## Usage

```rust
use cctv_detect::{Detection, Zone, ZoneConfig};

let zone = Zone::rectangle(0.1, 0.1, 0.8, 0.8);
let config = ZoneConfig::default_for(zone.clone(), 0.75);
let detection = Detection::motion(zone, 0.85);

assert!(detection.confidence() >= 0.75);
```
