# Camera types

`CameraType` is *what the device is*, not *how it speaks*. Speaking a protocol
is the driver's job; the type is a value the whole fleet shares.

Source of truth: [`crates/cctv-core/src/types.rs`](../crates/cctv-core/src/types.rs).

## The six types

Order is load-bearing. `CameraType::order` fixes a stable, committee-ordered
**coarse → specific** sequence that the fleet reconciler relies on, and
`CameraType::all` returns the same sequence.

| Order | Variant | Shape |
| --- | --- | --- |
| 0 | `Consumer` | Consumer IP camera. WiFi / PoE. Minimal PTZ, typical 2.8 mm–4 mm lens. |
| 1 | `Business` | Business / retail / lobby. 3–8 mm lens, timeshift, configurable overlay. |
| 2 | `Ptz` | Purpose-built PTZ. Heavy pan / tilt / zoom, accessible remote or app. |
| 3 | `Indoor` | Fixed indoor camera. |
| 4 | `Outdoor` | Outdoor (IP66 / IK10). Needs signal-breach, night / vandal logic. |
| 5 | `Ai` | AI camera with a dedicated detection payload and badge reader. |

## Rules the reconciler and the facade depend on

- **`CameraType` is a value, not a trait.** Drivers **must not** add variants.
  The taxonomy is closed; extending it is a `cctv-core` change.
- **PTZ is a capability, not a type.** `ptz_supported` on `ProbeResult` says
  whether a device can pan / tilt — a `Business` camera may.
- **AI is a capability, not a type.** A camera can be `Consumer` *and* have AI
  detection. `Ai` marks the purpose-built devices, not every AI-capable one.
- **Order is stable.** `Consumer → Business → Ptz → Indoor → Outdoor → Ai` is the
  coarse-to-specific sequence; do not reorder for aesthetics.

Why this shape: keeping capabilities out of the type means the reconciler asks
"can this device do X?" against `ProbeResult` rather than pattern-matching on a
type that would then need a variant for every combination.

---

**License:** MIT — see [`LICENSE`](../LICENSE).
**Security:** see [`SECURITY.md`](../SECURITY.md).
