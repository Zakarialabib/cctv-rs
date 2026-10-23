# CCTV Platform Spec — cctv-rs

> **Status:** v1 draft · 2026-10-23
> **Scope:** camera/fleet backend as a pure-Rust crate, mirroring the `tv-rs` pattern
> **Primary sources (this repo):** `docs/02-BACKEND/02-commands-reference.md`,
> `docs/02-BACKEND/01-ipc-contract.md`, `docs/01-ARCHITECTURE/04-EVENT-BUS-AND-PROTOCOLS.md`,
> `docs/00-meta/00-INDEX.md`, `docs/04-FEATURES/` (use cases), `docs/05-DEVELOPMENT/04-testing.md`
> **Reference sibling:** `tv-rs` (crate-layout pattern, `C:\laragon\www\tv-rs`)

---

## 0. Summary

`cctv-rs` is the **pure-Rust CCTV backend** for SignageMaster. It owns every camera concern:
discovery, stream ingestion (RTSP/Mjpeg), PTZ, AI detection, zones, motion monitoring,
snapshots, presets, reconnection, health, and fleet-state reconciliation — while remaining a
**library with no UI, no Tauri, and no SQLite.** The Signage app consumes it through one small
facade (`cctv`), and the Ezviz Open Platform reverse-engineered client
(`ezviz-openapi-rs`) becomes a **driver crate** inside it, exactly as `tv-rs` keeps each
vendor protocol in its own crate.

The rule from `tv-rs`: *nothing panics in library code, and a capability a camera does not
have is a typed error, never `unimplemented!()` or `unwrap()`.*

---

## 1. The CCTV use surface (in / out)

Everything Signage can do with a camera, as **in → out** pairs:

| In | Out | Example |
|---|---|---|
| phone taps a camera tile | live frame, decoded | `get_camera_frame` — MJPEG/RTSP decode |
| operator starts playback | RTSP/MJPEG stream at target quality | `start_camera_stream` + `set_quality_profile` |
| phone aims and sends a key | PTZ move | `ptz_move { pan, tilt, zoom, speed }` |
| operator sets a preset | → snapshot of camera position | `camera_save_preset` / `camera_goto_preset` |
| person crosses a zone | motion/detection event → dashboard | `run_zone_detection`, `zone_motion_start_monitor` |
| AI overlay in dashboard | per-camera object/badge analysis | `camera_detect_objects`, `camera_ai_detection_set_enabled` |
| operator renames/disables a camera | updated settings persisted | `camera_update`, `camera_set_enabled`, `camera_config_set` |
| screen goes off the network | auto-reconnect + health | `monitor_camera_health`, `camera_get_reconnection_info` |
| camera frame arrives | typed event → dashboard + capture queue | `CameraFrameAnalyzed`, `MotionDetected` (event bus) |
| dashboard "snapshot" button | image saved to disk | `camera_save_snapshot` / `camera_get_snapshots` |

**The 4 sub-systems** that repeat across every vendor: *discover, stream, control (PTZ),
analyze + record (zones/AI/motion)*, plus the C-level plumbing (reconnection, config,
health, snapshots).

---

## 2. Architecture — the `cctv-rs` workspace

Mirrors `tv-rs`. Each crate is one customer of concern; nothing crosses except the type
boundary.

```text
┌──────────────────────────────────────────────────────────────┐
│ cctv (facade)                      SignageMaster consumes it   │
│  → discover_cameras() · get_camera_frame() · ptz_move()      │
│  → start_camera_stream() · camera_ai_detection_set_enabled() │
└───────────────┬──────────────────────────────────────────────┘
                │ CameraHandle + CommandId + CameraError
                ▼
┌──────────────────────────────────────────────────────────────┐
│ cctv-core          device model (NO tokio, NO protocol)      │
│  - CameraHandle · CommandId · CameraState · CameraEvent      │
│  - CameraError taxonomy · MockBackend · conformance suite    │
└───────────────┬──────────────────────────────────────────────┘
                │ traits / events
                ▼
┌───────────────┬───────────────┬───────────────┬───────────────┐
│cctv-discovery │cctv-transport │cctv-ezviz     │cctv-agent     │
│ SSDP/mDNS     │ TLS policy,   │ Ezviz Open   │ fleet recon-  │
│ discovery     │ budgeted    │ Open-API    │ ciliation   │
│             │ frame reader│ reverse-eng │ (offline→armed→streaming)│
└───────────────┴───────────────┴───────────────┴───────────────┘
```

**Crate responsibilities**

| Crate | Responsibility | Barely mentions another |
|---|---|---|
| `cctv-core` | device model, traits, state, errors, mock, conformance, quirks (no tokio/protocol) | — |
| `cctv` | facade: discovery orchestration, vendor identification, `CameraHandle` impl | `cctv-core`, maybe `cctv-ezviz` |
| `cctv-discovery` | camera discovery (unicast / multicast / USB) | `cctv-transport` |
| `cctv-transport` | TLS policy + pinning + budgeted frame reader | `cctv-core` |
| `cctv-ezviz` | Ezviz Open Platform driver (from `ezviz-openapi-rs`); the camera-facing surface | `cctv-transport`, maybe `cctv-core` |
| `cctv-agent` | reconciler: desired state → graded remediation, audit trail | `cctv-core` |
| `cctv-cli` | terminal front end | `cctv-core` |

## 4. Camera type taxonomy (the `CameraType` enum)

```
Consumer  ·  Business  ·  PTZ  ·  Indoor  ·  Outdoor  ·  AI  (badge reader / AI view)
        \_________________________/
                    all are "Camera" at the core level;
                    PTZ ships int PTZ ∈ [-90..+90], zoom steps;
                    Outdoor ships signal-breach logic; AI carries the config payload
```

A PTZ camera's `ZoomStep` is a discrete set; an outdoor camera's `ArmMode` is the union of
`ArmMode`: `Off`, `Day`, `Night`, `DuskDawn`, `Custom(Badge|Time|Motion)` — chosen from
settings, not from a protocol. **Types live in `cctv-core`; protocols only in drivers.**

## 5. Protocol matrix

| Capability | Cable | Transport | Auth | Who speaks it |
|---|---|---|---|---|
| LAN discovery | — | **mDNS/DNS-SD**, UDP 1900 & multicast | none | `cctv-discovery` |
| RTSP stream | Cat5e | `cctv-transport` (TCP + buffer) | token / cert pinning | driver |
| MJPEG/frame | — | **cctv-transport** | none | `cctv-ezviz` |
| ONVIF PTZ | Cat5e | HTTP + SOAP | user/pass | `cctv-ezviz` (optional) |
| Ezviz cloud API | Cloud | **ezviz-openapi-rs** (reverse-engineered) | token | `cctv-ezviz` |
| USB camera | — | kernel/hardware | none | app (not a cctv crate) |
| Frames → dashboard | — | Tauri IPC | typed Zod | seam (`useIpcQuery`) |
| Events | — | EventBus → bridge | typed | `cctv-core` publishes, app consumes |

**Rule (tv-rs law):** no vendor SDK ships with `cctv-core`; `cctv-ezviz` is the only driver
that touches the Ezviz protocol.

## 6. IPC seam (SignageMaster side)

SignageMaster owns the system; the frontend React owns the screen; they meet only at the
typed IPC seam — and CCTV lives there exactly like everything else:

- **In:** the frontend's typed command surface for cameras is the same
  `src/core/ipc/CommandRegistry.ts` + `src/core/ipc/useIpcQuery.ts` the app already has
  (`get_camera_frame`, `start_camera_stream`, `ptz_move`, `camera_set_enabled`,
  `camera_detect_objects`, `camera_get_stats`, `camera_test_all`, `camera_config_get/set`,
  `camera_get_presets`/`save_goto_delete_preset`, `camera_delete_snapshot`, `zone_*`,
  `camera_ai_detection_*`, `camera_get_reconnection_info`, ... ≈ 50 camera commands;
  Zod schemas live in `src/core/ipc/validation.ts`);
- **Out:** Rust publishes typed Tauri events (`CameraFrameAnalyzed`, `MotionDetected`,
  `CameraStreamHealth`, ...) through the `EventBus`, heard by the frontend via
  `useTauriEvent` / `TauriEventMap`;
- **Storage:** camera settings, presets, zones, snapshots live in `signagemaster.db`
  (SQLite WAL), namespaces per crate — camera config in a `camera-*` namespace;
- **Rule R1 (seam isolation):** only `src/core/adapters/**` or the customer's
  `adapters/**` may call `invoke()` directly. The facade is the whole bridge.

## 7. Event taxonomy (rid of the 11-procotol doc)

| Event | Emitted by | Consumed by |
|---|---|---|
| `CameraFrameAnalyzed` | `cctv-core` → driver | dashboard overlay, `device-hardware`-style analytics |
| `MotionDetected` | `cctv-core` | zone alarms, `device-hardware`-style monitoring |
| `CameraStreamHealth` | `cctv-core` | dashboard heartbeat, `camera_test_all` |
| `CameraReconnected` / `CameraDropped` | `cctv-core` | fleet status |

## 8. Data model (SQLite WAL, so the *driver* is protocol-only)

| Namespace | Tables |
|---|---|
| camera-config | per-camera settings, motion config, presets, zones |
| camera-stats | last frame stats, stats history, reconnection info |
| camera-snapshot | path/name, captured_at, checksum |
| camera-events | typed event log (`CameraFrameAnalyzed`, `MotionDetected`, `StreamHealth`) |

## 9. Implementation plan

| Phase | Deliverable | Gate |
|---|---|---|
| 1 | `cctv-core`: traits + state + error + mock + conformance | `cargo check -p cctv-core` clean, no tokio |
| 2 | `cctv-transport` + `cctv-discovery` | compile |
| 3 | `cctv-ezviz` wraps `ezviz-openapi-rs` (token, device list, types) | `cargo test -p cctv-ezviz` (wiremock) |
| 4 | `cctv` facade + rewrite `src/core/cctv.ts` surface | `cargo check -p cctv` |
| 5 | `cctv-agent` reconciler (optional, after V1) | `cargo test -p cctv-agent` |
| 6 | Integration *with* the app (no vendor in the app) | `grep -ri ezviz src/` empty |

## 10. Decisions (locked — user confirmed 2026-10-23)

1. **Owner / repo** — `cctv-rs` is the user's workspace (`C:\laragon\www\cctv-rs`), published standalone.
2. **Layout** — in `crates/` — `Simple-Signage/crates/cctv-rs/...` joins the refactored crate tree.
3. **Ezviz** — folded into `cctv-rs` as **one** crate (`cctv-ezviz`); the standalone `ezviz-openapi-rs/` folder is kept
   because it is published standalone — `cctv-ezviz` depends on it via path dep, never copies code.
4. **Ranges owned** — `cctv-rs` owns Ezviz, Hikvision, Dahua, and others: one driver crate per
   vendor (driver crate = protocol only).
5. **Publish scope** — `cctv-core` ships the camera types/commands **now**: traits,
   `CameraHandle`, `CameraError`, conformance — the public device model.
6. **App facade** — `src/core/cctv.ts` becomes a pure facade over `cctv-core`; no vendor names.

## 11. Open items

- PTZ protocol negotiation against a live camera (driver work — deferred to V2).
- Live stream serving (`streaming-core` repo is a separate, planned repo).
- `cctv-rs` is currently an empty directory; this spec defines its contents.
- The Ezviz reverse-engineered surface is what the client exposes; lock it down before
  writing more than passive reads into the driver.

## 12. Acceptance criteria

- `cargo check --workspace` exits 0 (with the app's new membership).
- `src/core/cctv.ts` has no `ezviz` / no vendor names.
- `camera_get_frame`, `start_camera_stream`, `ptz_move`, `camera_config_get/set`,
  `camera_get_presets`/`save_goto_delete_preset`, `camera_get_reconnection_info` are all
  available as facade commands and typed in the registry + Zod schemas.
- Conformance suite passes on the mock backend.
- `cargo tree -p cctv-core` shows only std + serde.

---

## Decisions — user confirmed 2026-10-23 (locked)

1. **ezviz folding** — `cctv-rs` folds `ezviz-openapi-rs` into **one** crate (`cctv-ezviz`);
   the original `ezviz-openapi-rs/` folder is **kept** as a standalone published crate
   (path dep from `cctv-ezviz`, never copied code). ✓ *"folded into cctv-rs as one crate,
   keep original folder"*
2. **Publish scope** — `cctv-core` ships the camera types/commands **now**: traits,
   `CameraHandle`, `CameraError`, conformance — the public device model. ✓ *"cctv-core now"*
3. **Layout** — `cctv-rs` lives in `Simple-Signage/crates/cctv-rs/` (joins the refactored
   crate tree). ✓ *"in crates/"*
4. **Ranges owned** — `cctv-rs` owns Ezviz, Hikvision, Dahua, and others: one driver crate
   per vendor (driver crate = protocol only). ✓ *"ezviz, hikvision, dahua, or others"*
5. **App facade** — `src/core/cctv.ts` becomes a pure facade over `cctv-core`; no vendor
   names. ✓ *"the app's cctv.ts becomes a pure facade"*

[PLACEHOLDER]
> Owner name: `<owner-name>` · Date: `<date>` · Reviewer: `<reviewer>` ·
> Target branch: `<branch-name>` · Tag line: `<one-line summary>`

---

## Appendix A — Command taxonomy (from `docs/02-BACKEND/02-commands-reference.md`)

**Camera base (34):** `discover_cameras`, `discover_cameras_by_range`, `list_usb_cameras`,
`live_source_list`, `live_source_delete`, `live_source_save`, `live_source_get`,
`test_camera_connection`, `get_camera_frame`, `start_camera_stream`,
`set_global_frame_rate`, `set_quality_profile`, `monitor_camera_health`,
`scan_for_new_cameras`, `get_camera_settings`, `update_camera_settings`,
`camera_detect_objects`, `ptz_move`, `camera_update`, `camera_set_enabled`,
`camera_set_critical`, `camera_reorder`, `camera_get_stats`,
`camera_get_all_stats`, `camera_test_all`, `camera_get_presets`,
`camera_save_preset`, `camera_goto_preset`, `camera_delete_preset`,
`camera_screenshot`, `camera_reset_settings`, `zone_motion_delete_event`,
`zone_motion_clear_events`, `camera_get_available_models`

**AI (2):** `camera_ai_detection_set_enabled`, `camera_ai_detection_get_enabled` — plus
`camera_get_available_models`, `camera_set_detection_model`.

**MJPEG (4):** `mjpeg_get_status`, `mjpeg_get_endpoint_url`, `mjpeg_restart`, `mjpeg_set_enabled`.

**Zones (4):** `zone_list`, `zone_save`, `zone_delete`, `run_zone_detection`.
**Zone motion (6):** `zone_motion_get_config`, `zone_motion_save_config`,
`zone_motion_start_monitor`, `zone_motion_stop_monitor`, `zone_motion_list_events`,
`zone_motion_acknowledge_event`.

**Snapshots (4):** `camera_save_snapshot`, `camera_delete_snapshot`, `camera_get_snapshots`,
`camera_snapshot_exists`. **Presets (3):** `camera_save_preset`, `camera_goto_preset`,
`camera_delete_preset`. **Reconnection (1):** `camera_get_reconnection_info`.
**Config (2):** `camera_config_set`, `camera_config_get`. **Message queue (1):**
`camera_pop_corrected_image`. **Composite (2):** `camera_start_composite`,
`camera_stop_composite`. **Stream health (1):** `camera_get_monitor_health`.

## Appendix B — Protocol families (from `docs/01-ARCHITECTURE/04-EVENT-BUS-AND-PROTOCOLS.md`)

1. Tauri IPC (typed RPC)
2. HTTP/HTTPS egress (REST/JSON/Binary)
3. WebSockets (WSS/WS)
4. P2P mesh
5. mDNS/DNS-SD
6. DLNA/UPnP
7. HLS server
8. **Camera RTSP / MJPEG** ← the camera protocol
9. JSON-RPC over stdio
10. Webhooks (HMAC-SHA256)
11. Local LLM / ONNX
