# cctv-detect

[![Crates.io](https://img.shields.io/crates/v/cctv-detect.svg)](https://crates.io/crates/cctv-detect)
[![docs.rs](https://docs.rs/cctv-detect/badge.svg)](https://docs.rs/cctv-detect)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Detection — the device model for motion / AI detection in `cctv-rs`. Zones, detection events, and the monitor settings a user reads and saves. Deliberately thin: free of I/O and free of algorithms — the camera driver owns the frame, the application owns the inference, and this crate carries the *what*.

## What it is

A detection is a **report, not a command**: a camera tells the fleet where it saw motion, and the fleet decides what to do about it.

| Type | Role |
| --- | --- |
| `Detection` | One detection event (motion blob or object) with zone, class, confidence and optional bounding box. Confidence is clamped to `0.0..1.0` on construction. |
| `Zone` | The boundary a detection fired inside — `point` (centred box in native pixels) or `rectangle` (normalised `0.0..1.0`), each field clamped on its own. |
| `ZoneConfig` | The detection monitor settings: minimum confidence, minimum detections per window, window length. |

## No panics

The public surface is total on its inputs: a `ZoneConfig` that asks for a window wider than the frame is clamped, not rejected, and a `None` threshold is a backstop, not a caller error.

## Usage

```rust
use cctv_detect::{Detection, Zone, ZoneConfig};

let zone = Zone::rectangle(0.25, 0.25, 0.5, 0.5);
let config = ZoneConfig::default_for(zone, 0.8);
let det = Detection::motion(Zone::point(1.0, 1.0, 4.0), 0.9);
assert!(det.is_motion());
assert!(det.confidence >= config.threshold);
```

The facade wires this into the fleet: `Facade::zone_save` stores `ZoneConfig`s, `Facade::record_detection` gates detections on a zone's threshold and window, and fired detections surface as `MotionEvent`s through the `ZoneMotionListEvents` command.

## License

MIT
