# 04 — Camera Taxonomy

> `CameraType` models *what a device is*, independently of *how it communicates*.
> Protocol details belong to drivers; device categorization is shared across the
> fleet.

**Read this when** you are classifying a device, ordering a fleet, or deciding
whether a behaviour belongs on the type or on a capability flag.

Defined in [`crates/cctv-core/src/types.rs`](../crates/cctv-core/src/types.rs).

---

## 1. Classification Enum

`CameraType::order` defines a stable, coarse-to-fine sequence used during fleet
reconciliation:

| Order | Variant | Category description |
| --- | --- | --- |
| 0 | `Consumer` | Consumer IP camera (WiFi / PoE). Fixed view or entry-level pan/tilt. |
| 1 | `Business` | Commercial / retail / office camera. Fixed wide-angle lens, configurable privacy overlays. |
| 2 | `Ptz` | Pan-tilt-zoom dome or bullet camera with optical zoom and continuous positioning. |
| 3 | `Indoor` | Fixed indoor housing camera. |
| 4 | `Outdoor` | Weatherproof outdoor camera (IP66+ / IK10) with IR illuminators. |
| 5 | `Ai` | Specialized AI camera equipped with on-edge neural processing. |

---

## 2. Design Rules

1. **Closed value taxonomy.** `CameraType` is a concrete enum, not an extensible
   trait. New device classes require core model updates.
2. **Capabilities vs. types.** PTZ and AI capabilities (`ptz_supported`,
   `ai_supported`) are independent flags. A `Business` camera can support PTZ
   commands without changing its type.
3. **Deterministic sorting.** `CameraType::order` ensures identical reconciliation
   sequencing across multiple fleet scans.

---

## See also

- [03 — Command Catalogue](03-commands.md) — the commands a classified device accepts.
- [01 — Architecture](01-architecture.md) — where the device model sits in the crate graph.

---

## License & Security

- **License:** Apache-2.0 ([LICENSE](../LICENSE)).
- **Security:** see [SECURITY.md](../SECURITY.md) for the vulnerability disclosure
  process.
