# Camera Taxonomy

`CameraType` models *what a device is*, independently of *how it communicates*. Protocol specifics are handled entirely by drivers; device classification is shared across the fleet.

Source code reference: [`crates/cctv-core/src/types.rs`](../crates/cctv-core/src/types.rs).

---

## Classification Enum

`CameraType::order` defines a stable, committee-ordered coarse-to-specific sequence used during fleet reconciliation:

| Order | Variant | Target Hardware Category |
| --- | --- | --- |
| 0 | `Consumer` | Consumer IP camera (WiFi / PoE). Basic fixed view or entry-level pan/tilt. |
| 1 | `Business` | Commercial / retail / office camera. Fixed wide-angle lens, configurable overlays. |
| 2 | `Ptz` | Purpose-built Pan-Tilt-Zoom dome or bullet camera with high-speed optical zoom. |
| 3 | `Indoor` | Fixed indoor housing camera. |
| 4 | `Outdoor` | Weatherproof outdoor camera (IP66 / IK10 rated) with IR illuminators. |
| 5 | `Ai` | Specialized AI camera equipped with on-edge neural processing hardware. |

---

## Architectural Rules

1. **Closed Value Taxonomy:** `CameraType` is a concrete enum value, not a trait. Drivers cannot introduce new enum variants; taxonomy additions require core updates.
2. **Capabilities Are Distinct From Types:** PTZ and AI support are capabilities (`ptz_supported`, `ai_supported`), not mutually exclusive device types. A `Business` camera may support PTZ commands.
3. **Deterministic Order:** `Consumer → Business → Ptz → Indoor → Outdoor → Ai` provides a stable reconciliation order across fleet sweeps.
