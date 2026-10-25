# Architecture

`cctv-rs` is the **pure-Rust CCTV backend** for SignageMaster. It owns every camera
concern — discovery, stream ingestion (RTSP / MJPEG), PTZ, AI detection, zones,
motion monitoring, snapshots, presets, reconnection, health and fleet-state
reconciliation — while remaining a **library: no UI, no Tauri, no SQLite.**

The application consumes it through one facade crate. Each protocol is a **driver
crate** behind that facade — `cctv-ezviz` for the Ezviz Open Platform,
`cctv-onvif` for ONVIF Device / Media / PTZ — the same way `tv-rs` keeps each
vendor protocol in its own crate.

The rule inherited from `tv-rs`: *nothing panics in library code, and a capability
a camera does not have is a typed error — never `unimplemented!()` or `unwrap()`.*

---

## 1. The camera use surface

Everything the application can do with a camera, as **in → out** pairs:

| In | Out | Example |
| --- | --- | --- |
| a phone taps a camera tile | a decoded live frame | `get_camera_frame` |
| an operator starts playback | RTSP / MJPEG stream at the target quality | `start_camera_stream` + `set_quality_profile` |
| a phone aims and sends a key | PTZ move | `ptz_move { pan, tilt, zoom, speed }` |
| an operator sets a preset | the camera returns to that position | `camera_save_preset` / `camera_goto_preset` |
| a person crosses a zone | a motion / detection event | `run_zone_detection`, `zone_motion_start_monitor` |
| the dashboard wants an AI overlay | per-camera object analysis | `camera_detect_objects`, `camera_ai_detection_set_enabled` |
| an operator renames or disables a camera | updated, persisted settings | `camera_update`, `camera_set_enabled`, `camera_config_set` |
| a camera drops off the network | reconnection + health | `monitor_camera_health`, `camera_get_reconnection_info` |
| a frame arrives | a typed event | `CameraFrameAnalyzed`, `MotionDetected` |
| the dashboard asks for a snapshot | an image on disk | `camera_save_snapshot` / `camera_get_snapshots` |

Four sub-systems repeat across every vendor — **discover, stream, control (PTZ),
analyse and record (zones / AI / motion)** — plus the cross-cutting plumbing
(reconnection, config, health, snapshots).

---

## 2. Crates

Each crate has one reason to exist. Nothing crosses between them except the type
boundary in `cctv-core`.

```text
cctv-rs
├── cctv-core ───────────── serde, serde_json
├── cctv-transport ──────── serde
├── cctv-discovery ──────── cctv-core, cctv-transport
├── cctv-onvif ──────────── cctv-core, cctv-transport (http)
├── cctv-ezviz ──────────── cctv-core, ezviz-openapi   (published from its own repository)
└── cctv-agent ──────────── cctv-core
```

`cctv-rs` is the facade: it depends on all six and is the only crate the
application talks to.

| Crate | Depends on | Role |
| --- | --- | --- |
| `cctv-core` | `serde`, `serde_json` (+ optional `toml` behind `quirks`) | The device model: `CameraHandle`, `CommandId`, `CameraState`, the `CameraError` taxonomy, the mock backend and the conformance suite. No runtime, no protocol. Feature `quirks` adds the model/firmware defect table, as data. |
| `cctv-transport` | `serde` | TLS policy (pinning, minimum version, ALPN), the budgeted frame reader — bounded staleness and backlog, plus the stream measurements `camera_get_stats` reports — the shared XML scanner and (feature `http`) the rustls-only HTTP client. Policy is **data**. |
| `cctv-discovery` | `cctv-core`, `cctv-transport` | SSDP `M-SEARCH` and WS-Discovery `Probe` sweeps over UDP multicast. Emits `cctv-core` device-model items; USB enumeration is the application's. |
| `cctv-onvif` | `cctv-core`, `cctv-transport` | The ONVIF driver: Device / Media / PTZ SOAP builders and parsers with WS-Security auth, an `OnvifSession` that drives them over the shared HTTP client, and an `OnvifCameraBackend`. Protocol only — shared by every ONVIF vendor. |
| `cctv-ezviz` | `cctv-core`, `ezviz-openapi` | The Ezviz Open Platform driver: token, device list, stream, PTZ, presets, alarms, capture. `EzvizCameraBackend` implements `CameraBackend`, so the conformance suite covers it. |
| `cctv-agent` | `cctv-core` | The fleet reconciler: desired state → graded remediation (`Grade` / `Action`), with an append-only audit trail. |
| `cctv-rs` | all six | The facade: the command surface, the driver registry (`CameraConfig::driver` → a factory), the LAN sweep and fleet reconciliation behind one handle. |

### Dependency direction

- **`cctv-core` is the contract.** Everything depends on it; it depends on
  nothing in the workspace. It must stay embeddable by a consumer that links no
  async runtime, so it never gains tokio — CI fails the build if tokio appears in
  its dependency graph.
- **Drivers are protocol only.** `cctv-onvif` speaks ONVIF through
  `cctv-transport`; `cctv-ezviz` wraps the standalone `ezviz-openapi` crate
  through a git dependency pinned to a rev (published crates resolve the
  crates.io release instead). Neither copies vendor code and neither reaches the
  application directly.
- **The facade is the only public surface.** Vendor names must not appear in it.
- **The facade resolves drivers through data.** `CameraConfig::driver` is a key
  the facade's registry maps to a factory, so a vendor crate is reached by value,
  never by name.
- **Commands are addressed by `CommandId`** — the *what*. Drivers answer the *how*.
  See [commands.md](commands.md).

None of the seven crates pulls in tokio today; the tokio-free guarantee is enforced
for `cctv-core` because it is the one the application embeds.

---

## 3. Protocol matrix

| Capability | Transport | Auth | Who speaks it |
| --- | --- | --- | --- |
| LAN discovery | SSDP `M-SEARCH` (UDP 1900) + WS-Discovery `Probe` (UDP 3702), multicast | none | `cctv-discovery` |
| RTSP stream | TCP + buffering, via `cctv-transport` | token / certificate pinning | the driver |
| MJPEG frames | via `cctv-transport` | none | `cctv-ezviz` |
| ONVIF (device / media / PTZ) | HTTP + SOAP, via `cctv-transport` | WS-Security `UsernameToken` | `cctv-onvif` |
| Ezviz cloud API | HTTPS, through `ezviz-openapi` | token | `cctv-ezviz` |
| USB camera | kernel / hardware | none | the application, not a `cctv-*` crate |
| Frames → dashboard | Tauri IPC | typed schema | the integration seam |
| Events | in-process event bus | typed | `cctv-core` publishes, the application consumes |

**Rule:** no vendor SDK ships with `cctv-core`. `cctv-ezviz` is the only driver
that touches the Ezviz protocol, and `cctv-onvif` the only one that touches ONVIF.

---

## 4. Event taxonomy

| Event | Emitted by | Consumed by |
| --- | --- | --- |
| `CameraFrameAnalyzed` | `cctv-core` (via a driver) | the overlay / analytics consumer |
| `MotionDetected` | `cctv-core` | zone alarms, monitoring |
| `CameraStreamHealth` | `cctv-core` | the dashboard heartbeat, `camera_test_all` |
| `CameraReconnected` / `CameraDropped` | `cctv-core` | fleet status |

---

## 5. Storage model

The library owns no database. State that must persist is written by the
application, namespaced per concern so that a driver stays protocol-only:

| Namespace | Holds |
| --- | --- |
| `camera-config` | per-camera settings, motion config, presets, zones |
| `camera-stats` | last frame stats, stats history, reconnection info |
| `camera-snapshot` | path / name, `captured_at`, checksum |
| `camera-events` | the typed event log (`CameraFrameAnalyzed`, `MotionDetected`, `CameraStreamHealth`) |

---

## 6. Integration seam

The application and `cctv-rs` meet only at this boundary. The application side is
**not** in this repository; what follows is the contract this workspace is built
to satisfy.

- **In.** Camera commands enter through the facade, typed end to end. The
  command surface is the `CommandId` catalogue in [commands.md](commands.md) —
  roughly fifty operations today.
- **Out.** Typed events (section 4) are published through the application's event
  bus and consumed by the frontend.
- **Storage.** Camera settings, presets, zones, stats and snapshots live in the
  application's namespaces (section 5).
- **Seam isolation.** Only the application's adapter layer may call the facade
  directly. The facade is the whole bridge; nothing bypasses it.

---

## 7. Acceptance criteria

- `cargo check --workspace` exits 0.
- `cargo tree -p cctv-core` shows no tokio in its dependency graph.
- The conformance suite passes against the mock backend.
- The application surface carries no vendor name; the `ezviz` vocabulary stops at
  `cctv-ezviz` and the ONVIF vocabulary at `cctv-onvif`.
- The commands the application depends on — `get_camera_frame`,
  `start_camera_stream`, `ptz_move`, `camera_config_get/set`,
  `camera_get_presets` / save / goto / delete, `camera_get_reconnection_info` —
  are reachable through the facade and typed.
