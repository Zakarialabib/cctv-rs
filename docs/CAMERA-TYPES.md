# Camera Types

The type taxonomy a CMS embeds. These are *what a device is*, not how it talks
— that is the driver's job. PTZ, Indoor and Outdoor carry signal / power quirks;
AI carries the detection payload.

## Enumeration

| Variant | What it is | Typical use |
|---|---|---|
| `Consumer` | WiFi / PoE IP camera. Minimal PTZ, ~2.8mm–4mm lens. | Home / small office |
| `Business` | 3–8mm lens, timeshift, configurable overlay | Retail / lobby |
| `Ptz` | Purpose-built PTZ: heavy pan / tilt / zoom | Signage-chassis close-up camera |
| `Indoor` | Fixed indoor camera | Wall / ceiling mount |
| `Outdoor` | IP66 / IK10. Needs signal-breach, night / vandal logic | Entrance / yard |
| `Ai` | AI camera with dedicated detection payload + badge reader | People counting / access |

## Ordering

`Consumer → Business → Ptz → Indoor → Outdoor → Ai` — the stable order the
fleet reconciler uses (coarse → specific). Do not reorder without a committee.

## Modeling rules

- `CameraType` is a value, not a trait. Drivers **must not** add variants.
- PTZ is a *capability* (`ptz_supported` in `ProbeResult`), not part of the
  type. A `Business` camera may pan / tilt.
- AI is a *capability*, not a type. A camera can be `Consumer` and have AI.
- Examples of capabilities: `rtsp_supported`, `mjpeg_supported`,
  `ptz_supported`, `ai_supported`, `badge_supported`, `motion_zone_supported`.
