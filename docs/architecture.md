# System Architecture

`cctv-rs` is a **pure-Rust CCTV backend engine**. It handles camera discovery, stream ingestion (RTSP / MJPEG), PTZ movement, AI detection, zone monitoring, snapshot generation, and fleet reconciliation — operating as a **pure library with no UI, no async runtime lock-in, and no database dependencies**.

Applications consume `cctv-rs` through a single aggregate facade crate ([`cctv-rs`](../crates/cctv-rs)). Protocols and vendor APIs live in individual **driver crates** behind that facade (`cctv-onvif` for ONVIF SOAP services, `cctv-ezviz` for Ezviz Open Platform).

**Engineering Invariant:** *Nothing panics in library code. Unsupported camera capabilities return explicit typed errors (`CameraError::Unsupported`) — never `unimplemented!()` or `unwrap()`.*

---

## 1. Application Use Surface

Everything an application does with cameras, modeled as **in → out** operations:

| Request | Response | Facade Entry Point |
| --- | --- | --- |
| Decode live frame | Decoded image snapshot | `get_camera_frame` |
| Start stream | Active RTSP / stream handle | `start_camera_stream` |
| PTZ move command | Unit execution status | `ptz_move { pan, tilt, zoom, speed }` |
| Save / Goto preset | Preset location confirmation | `camera_save_preset` / `camera_goto_preset` |
| Zone motion detection | Motion alert event | `run_zone_detection`, `zone_motion_start_monitor` |
| AI object analysis | Bounding boxes & object list | `camera_detect_objects`, `camera_ai_detection_set_enabled` |
| Configuration update | Persisted config state | `camera_update`, `camera_set_enabled`, `camera_config_set` |
| Camera drop / reconnection | Reconnection health state | `monitor_camera_health`, `camera_get_reconnection_info` |
| Disk snapshot capture | Local image file metadata | `camera_save_snapshot` / `camera_get_snapshots` |

---

## 2. Crate Topology & Dependency Graph

```text
cctv-rs (Aggregate Facade)
├── cctv-core ───────────── serde, thiserror, async-trait (alloc-only)
├── cctv-transport ──────── serde, tokio-rustls
├── cctv-rtp ────────────── (zero dependencies)
├── cctv-rtsp ───────────── base64
├── cctv-discovery ──────── cctv-core, cctv-transport
├── cctv-onvif ──────────── cctv-core, cctv-transport (http)
├── cctv-ezviz ──────────── cctv-core, ezviz-openapi
└── cctv-agent ──────────── cctv-core
```

### Roles & Responsibilities

| Crate | Direct Dependencies | Responsibility |
| --- | --- | --- |
| `cctv-core` | `serde`, `thiserror`, `async-trait` | Device model: `CameraHandle`, `CommandId`, `CameraState`, `CameraError`, mock backend, and conformance suite. `alloc`-only capable. Zero async runtime dependencies. |
| `cctv-transport` | `serde` (+ optional `tokio-rustls`) | TLS policy as data, budgeted `FrameReader`, zero-allocation XML scanner, and socket layer (`net` feature). |
| `cctv-rtp` | *(none)* | RTP header parsing, H.264 / H.265 access-unit depacketization (STAP-A, FU-A, AP, FU). Pure & dependency-free. |
| `cctv-rtsp` | `base64` | RTSP request/response framing, SDP parsing, and `$`-interleaved RTP stream demuxing. Pure codec. |
| `cctv-discovery` | `cctv-core`, `cctv-transport` | SSDP `M-SEARCH` and WS-Discovery `Probe` sweeps over UDP multicast. |
| `cctv-onvif` | `cctv-core`, `cctv-transport` | ONVIF Device / Media / PTZ SOAP builders and parsers with WS-Security `UsernameToken` authentication. |
| `cctv-ezviz` | `cctv-core`, `ezviz-openapi` | Ezviz Open Platform cloud driver wrapping account, stream, PTZ, and alarm endpoints. |
| `cctv-agent` | `cctv-core` | Deterministic fleet reconciler: compares desired vs observed state and issues graded remediation actions with an audit trail. |
| `cctv-rs` | all workspace crates | Public facade: aggregate command registry, driver factory, network discovery, and fleet reconciliation behind one handle. |

---

## 3. Protocol Matrix

| Capability | Transport | Auth | Driver |
| --- | --- | --- | --- |
| Network Discovery | SSDP `M-SEARCH` (UDP 1900) + WS-Discovery `Probe` (UDP 3702) multicast | Unauthenticated | `cctv-discovery` |
| RTSP Streaming | TCP / UDP socket via `cctv-transport`; codecs via `cctv-rtsp` & `cctv-rtp` | RTSP Digest / Basic | `cctv-rtsp` / Driver |
| ONVIF Management | HTTP + SOAP 1.2 via `cctv-transport` | WS-Security `UsernameToken` | `cctv-onvif` |
| Ezviz Cloud API | HTTPS via `ezviz-openapi` | Account Session Token | `cctv-ezviz` |
| USB Video Devices | Kernel / OS device API | System permissions | Consumed directly by host application |

---

## 4. Event Taxonomy

| Event | Publisher | Consumer |
| --- | --- | --- |
| `CameraFrameAnalyzed` | Driver / Core | Analytics & bounding box overlays |
| `MotionDetected` | Driver / Core | Alarm engine & zone monitors |
| `CameraStreamHealth` | Driver / Core | Application health monitor |
| `CameraReconnected` / `CameraDropped` | Core | Fleet state manager |

---

## 5. Storage Model

`cctv-rs` owns no internal database. State persistence is managed by the host application using distinct namespaces:

| Storage Namespace | Information Held |
| --- | --- |
| `camera-config` | Per-camera settings, network parameters, PTZ presets, detection zones |
| `camera-snapshot` | Image file paths, timestamps, resolution, payload checksums |
| `camera-events` | Persisted event logs (`MotionDetected`, `CameraFrameAnalyzed`, state drops) |

---

## 6. Host Integration Seam

Host applications interface with `cctv-rs` strictly at the facade boundary:

1. **Inbound Commands:** Applications issue typed commands via `Facade::execute(camera_id, CommandId)` or convenience methods (`get_camera_frame`, `ptz_move`).
2. **Outbound Events:** Drivers emit typed events consumed by application event channels.
3. **Driver Isolation:** Vendor-specific code and terminology stop at the respective driver crates. The host application interacts exclusively with `cctv-core` types.
