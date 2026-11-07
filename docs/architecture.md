# System Architecture

`cctv-rs` is a pure-Rust CCTV backend engine for discovering, streaming, controlling, and reconciling camera fleets. It operates purely as a library: no UI, no database dependencies, no async runtime lock-in.

Host applications consume `cctv-rs` via the aggregate facade crate ([`cctv-rs`](../crates/cctv-rs)). Protocols, transports, and vendor adapters live behind this facade.

```text
cctv-rs (Aggregate Facade)
├── cctv-core ───────────── alloc-only contract (traits, state, errors)
├── cctv-transport ──────── TLS policy, FrameReader, XML scanner, net sockets
├── cctv-rtp ────────────── RTP header & H.264/H.265 depacketizer (pure)
├── cctv-rtsp ───────────── RTSP framing, SDP parser, interleaved demux, Digest
├── cctv-driver ─────────── Vendor-neutral adapter seam: DriverAdapter, registry,
│                           discovery sweeps, plus the Ezviz (feature `ezviz`)
│                           and local-video UVC/PTP/DDC (features `uvc`,
│                           `uvc-ptp`, `ddc`) adapters
├── cctv-onvif ──────────── ONVIF SOAP services (Device/Media/PTZ)
├── cctv-agent ──────────── Fleet reconciler, StreamMux, SnapshotProxy
├── cctv-detect ─────────── Detection & zone configuration models
├── cctv-rag ────────────── Incident ledger, spatial topology, BM25 RAG retrieval
├── cctv-pair ───────────── Pairing ceremony, trust store, zbase32 fingerprints
└── cctv-sync ───────────── Registry convergence (CRDT: LWW register, OR-set)
```

---

## 1. Application Interface Surface

All camera interactions pass through the typed `Facade` interface:

| Intent | Response | Facade Entry Point |
| --- | --- | --- |
| Decode live frame | Decoded snapshot bytes | `get_camera_frame(handle)` |
| Start stream | Active stream descriptor | `start_camera_stream(handle)` |
| Move PTZ | Unit execution status | `ptz_move(handle, cmd)` |
| Preset operations | Preset confirmation / list | `camera_save_preset`, `camera_goto_preset`, `camera_delete_preset` |
| Zone motion events | Event list | `execute(handle, CommandId::ZoneMotionListEvents)` |
| Configure zones | Persisted zone setting | `zone_save(config)`, `zone_list()`, `zone_delete(id)` |
| Record detection | Evaluation against threshold | `record_detection(handle, detection)` |
| Reconcile fleet | Graded remediation actions | `reconcile(desired, observed)` |
| Universal dispatch | Strongly typed payload | `execute(handle, CommandId)` |

---

## 2. Crate Responsibilities

| Crate | Dependencies | Responsibility |
| --- | --- | --- |
| `cctv-core` | `thiserror`, `async-trait` (`serde` optional) | The immutable device model: `CameraHandle`, `CommandId`, `CameraState`, `CameraError`, quirks table, mock backend. `#![no_std]` + `alloc` compatible. |
| `cctv-transport` | `thiserror` (`tokio-rustls`, `reqwest` optional) | Data-driven TLS policy, time-budgeted `FrameReader`, zero-allocation XML scanner, TCP/TLS socket helpers. |
| `cctv-rtp` | *(none)* | Pure byte-slice RTP header parsing and H.264/H.265 access-unit reassembly (STAP-A, FU-A, AP, FU). |
| `cctv-rtsp` | `base64`, `zeroize` | Pure message framing, SDP track extraction, `$`-interleaved frame demuxing, and RFC 2617/7616 Digest auth state machine. |
| `cctv-driver` | `cctv-core` (+ `cctv-transport` under `discovery`) | Vendor-neutral adapter seam: the `DriverAdapter` factory surface, `DriverRegistry`, and the bundled protocol adapters. `discovery` (default) carries the SSDP `M-SEARCH` / WS-Discovery `Probe` sweeps; `ezviz` bundles the Ezviz Open Platform adapter; `uvc` / `uvc-ptp` / `ddc` bundle the local-video seam (mockable OS platform, UVC capture, PTP fetch, HAL hot-plug, DDC/CI display control). |
| `cctv-onvif` | `cctv-core`, `cctv-transport`, `cctv-driver` | SOAP 1.2 client for ONVIF Device, Media, and PTZ services; skewed-clock WS-Security `UsernameToken` generation; discovery delegation to `cctv-driver`. |
| `cctv-agent` | `cctv-core`, `cctv-rtsp` | Deterministic fleet reconciler, graded remediation actions (`Hold`, `MarkOffline`, `TakeOutOfService`), `StreamMux` GOP caching, and `SnapshotProxy`. |
| `cctv-detect` | *(pure / alloc-only)* | Motion and object detection primitives: `Detection`, `Zone` (`point`, `rectangle`), `ZoneConfig` with clamped normalization. |
| `cctv-rag` | `cctv-core`, `cctv-detect` | Deterministic retrieval layer: temporal incident ledger, spatial topology graph, BM25 index, and diagnostic playbooks, exposed as one `KnowledgeBaseIndex`. |
| `cctv-pair` | `cctv-core` (`serde-sync`) | Device pairing ceremony and trust store: the AKE handshake, `TrustStore` persistence, pairing-URI parsing, and `zbase32` human-comparable fingerprints. |
| `cctv-sync` | `cctv-core`, `cctv-pair` | Peer-state convergence: CRDT primitives (`LwwRegister`, `OrSet`) and the device/trust registry replicas derived from them. |
| `cctv-rs` | All workspace crates | Aggregate facade: unified driver factory registry, lifecycle management, zone processing, and discovery entry points. |

---

## 3. Protocol & Transport Matrix

| Channel | Wire Protocol | Auth / Security | Handler Crate |
| --- | --- | --- | --- |
| Discovery | UDP Multicast (`239.255.255.250:1900`, `:3702`) | Unauthenticated | `cctv-driver` (`discovery`) |
| RTSP Streaming | TCP / UDP RTSP 1.0 + interleaved RTP | HTTP Digest / Basic (RFC 2617 / RFC 7616) | `cctv-rtsp`, `cctv-rtp` |
| ONVIF Control | HTTP/1.1 SOAP 1.2 XML | WS-Security `UsernameToken` (clock-skew compensated) | `cctv-onvif` |
| Cloud Services | HTTPS REST JSON | OAuth / Account Access Tokens | `cctv-driver` (`ezviz`) |
| Local Video | USB / V4L2 / AVFoundation via OS Seam | OS permissions | `cctv-driver` (`uvc`) |

---

## 4. Key Architectural Guarantees

1. **Forbid Unsafe**: Every crate compiles with `#![forbid(unsafe_code)]`. Memory safety is enforced by the compiler.
2. **Panic-Free Operation**: No `unwrap()`, `expect()`, or panicking macros in library paths. Gaps return `CameraError::Unsupported`.
3. **Decoupled Runtimes**: `cctv-core` never links Tokio. Host applications choose their own execution environment; drivers bind async runtimes only when network I/O is enabled.
4. **Zeroized Memory**: Passwords and keys are held in `zeroize::Zeroizing<String>` containers, wiped on drop, and redacted from `Debug` logs.
5. **Separation of Inference**: `cctv-detect` models zones and results without ML runtime dependencies. Inference models run externally; detections are fed back into the facade.
