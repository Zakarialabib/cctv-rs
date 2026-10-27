# Target Architecture & Evolution Plan

Aspirational crate evolution, gap analysis, and workspace refinement goals.

---

## Crate Evolution Map

| Crate | Target Contract | Target Transport |
| --- | --- | --- |
| `cctv-core` | Pure `alloc`-only device contract (`serde`, `thiserror`, `async-trait`). | Zero I/O, zero network runtime dependencies. |
| `cctv-transport` | Data-driven TLS policy, time-budgeted `FrameReader`, zero-allocation XML scanner. | Pure TCP/UDP socket management (`net` feature). |
| `cctv-rtsp` | Generic RTSP `CameraBackend` driver implementation. | Socket connection management for DESCRIBE → SETUP → PLAY streams. |
| `cctv-rtp` | Zero-dependency RTP header and H.264/H.265 access-unit depacketizer. | Pure byte-slice processing. |
| `cctv-onvif` | ONVIF SOAP/HTTP `CameraBackend` + WS-Discovery integration. | Shared HTTP client via `cctv-transport`. |
| `cctv-agent` | Async fleet state reconciler with graded remediation. | Command issue & state drift remediation. |
| `cctv-rs` | Public aggregate facade with dynamic driver registry (`Arc<dyn CameraBackend>`). | Unified API entry point for host applications. |

---

## Key Design Decisions

1. **`alloc`-Only Core (`cctv-core`):** Removed untyped JSON DOMs (`serde_json::Value`) from core contract types to enable `#![no_std]` + `alloc` support.
2. **`thiserror` & `async-trait` Adoption:** Declarative error definitions and async trait interfaces across all drivers.
3. **`cctv-discovery` Coexistence:** Kept `cctv-discovery` as a shared UDP sweep crate so non-ONVIF RTSP cameras are discovered alongside ONVIF hardware.
4. **`Arc` Driver Registry:** The facade registry holds `Arc<dyn CameraBackend>` handles to allow concurrent command dispatch, health monitoring, and stream reading.
5. **Feature-Gated Socket Layer:** `cctv-transport` provides a `net` feature using Tokio and Rustls, allowing drivers to operate without heavy external HTTP dependencies.
