# Camera Taxonomy

`CameraType` models *what a device is*, independently of *how it communicates*. Protocol details belong to drivers; device categorization is shared across the fleet.

Defined in [`crates/cctv-core/src/types.rs`](../crates/cctv-core/src/types.rs).

---

## Classification Enum

`CameraType::order` defines a stable, coarse-to-fine sequence used during fleet reconciliation:

| Order | Variant | Category Description |
| --- | --- | --- |
| 0 | `Consumer` | Consumer IP camera (WiFi / PoE). Fixed view or entry-level pan/tilt. |
| 1 | `Business` | Commercial / retail / office camera. Fixed wide-angle lens, configurable privacy overlays. |
| 2 | `Ptz` | Pan-Tilt-Zoom dome or bullet camera with optical zoom and continuous positioning. |
| 3 | `Indoor` | Fixed indoor housing camera. |
| 4 | `Outdoor` | Weatherproof outdoor camera (IP66+ / IK10) with IR illuminators. |
| 5 | `Ai` | Specialized AI camera equipped with on-edge neural processing. |

---

## Design Rules

1. **Closed Value Taxonomy**: `CameraType` is a concrete enum, not an extensible trait. New device classes require core model updates.
2. **Capabilities vs. Types**: PTZ and AI capabilities (`ptz_supported`, `ai_supported`) are independent flags. A `Business` camera can support PTZ commands without changing its type.
3. **Deterministic Sorting**: `CameraType::order` ensures identical reconciliation sequencing across multiple fleet scans.
