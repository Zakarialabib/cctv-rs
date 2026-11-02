# Target Architecture

Aspirational crate evolution, boundaries, and architectural invariants for the `cctv-rs` workspace.

---

## Crate Contract Evolution

| Crate | Contract Boundary | Transport & Dependencies |
| --- | --- | --- |
| `cctv-core` | Pure `alloc`-only device contract (`serde-sync`, `thiserror`, `async-trait`). | Zero I/O, zero network sockets, zero Tokio. |
| `cctv-transport` | Data-driven TLS policy, time-budgeted `FrameReader`, zero-allocation XML scanner. | TCP/TLS socket abstractions (`net` feature, `rustls` only). |
| `cctv-rtp` | Zero-dependency RTP header and H.264/H.265 access-unit depacketizer. | Pure byte-slice processing. |
| `cctv-rtsp` | Pure message codecs, SDP parser, and Digest auth state machine. | Independent of socket I/O; consumed by streaming loops. |
| `cctv-discovery` | UDP multicast sweeps (SSDP and WS-Discovery). | Stateless discovery returning strongly typed `DiscoveredCamera`. |
| `cctv-onvif` | ONVIF SOAP/HTTP `CameraBackend` with clock-skew auth. | Transport-backed HTTP requests with XML parsing. |
| `cctv-ezviz` | Cloud `CameraBackend` wrapping `ezviz-openapi`. | Token lifecycle and stream endpoint translation. |
| `cctv-stream-uvc` | Ports & Adapters local video seam (UVC/PTP/HAL). | Isolated behind platform abstraction for hardware-free testability. |
| `cctv-agent` | Deterministic fleet state reconciler, `StreamMux`, `SnapshotProxy`. | Command issuance via `CommandRelay` and NALU broadcast caching. |
| `cctv-detect` | Motion & AI detection domain model. | Alloc-only; free of ML frameworks and I/O. |
| `cctv-rs` | Public aggregate facade with dynamic driver factory registry (`Arc<dyn CameraBackend>`). | Single unified interface for host applications. |

---

## Core Invariants

1. **`alloc`-Only Core (`cctv-core`)**: Contract types avoid heap-heavy untyped JSON structures (`serde_json::Value`), ensuring compatibility with constrained runtime environments.
2. **Explicit Workspace Manifest**: Member crates are listed explicitly in root `Cargo.toml` in dependency/publish order. No wildcard globs (`crates/*`).
3. **No Network Leak in Core**: Core types (`cctv-core`) must never import or leak network socket or Tokio types.
4. **Data-Driven TLS Policy**: TLS policies are plain data structures rather than runtime-toggled globals, allowing per-camera self-signed certificate overrides.
5. **Facade Boundaries**: Host applications communicate through `Facade` and `cctv-core` types, preventing vendor protocol leakage into business logic.
