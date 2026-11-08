# 01 — System Architecture

> `cctv-rs` is a pure-Rust CCTV backend engine for discovering, streaming,
> controlling, and reconciling camera fleets. It is a library: no UI, no database
> dependencies, no async-runtime lock-in.

**Read this when** you are orienting in the workspace — what the crates are, how
they depend on each other, and which invariants bind them. For the process rules
these crates are held to, see [02 — Engineering Standard](02-rust-package-standard.md);
for what ships when, see [07 — Execution Plan](07-plan.md).

Host applications consume the workspace through the aggregate facade crate
([`cctv-rs`](../crates/cctv-rs)). Protocols, transports, and vendor adapters live
behind that facade.

---

## 1. Crate Topology

```text
cctv-rs (Aggregate Facade)
├── cctv-core ───────────── alloc-only contract (traits, state, errors)
├── cctv-transport ──────── TLS policy, FrameReader, XML scanner, net sockets
├── cctv-rtp ────────────── RTP header & H.264/H.265 depacketizer (pure)
├── cctv-rtsp ───────────── RTSP framing, SDP parser, interleaved demux, Digest
├── cctv-driver ─────────── Vendor-neutral adapter seam: DriverAdapter, registry,
│                           discovery sweeps and the ONVIF SOAP services
│                           (feature `onvif`), plus the Ezviz (feature `ezviz`)
│                           and local-video UVC/PTP/DDC (features `uvc`,
│                           `uvc-ptp`, `ddc`) adapters
├── cctv-agent ──────────── Fleet reconciler, StreamMux, SnapshotProxy
├── cctv-detect ─────────── Detection & zone configuration models
├── cctv-rag ────────────── Incident ledger, spatial topology, BM25 RAG retrieval
├── cctv-pair ───────────── Pairing ceremony, trust store, zbase32 fingerprints
└── cctv-sync ───────────── Registry convergence (CRDT: LWW register, OR-set)
```

Dependency direction is strictly downward: the facade depends on every crate, and
no crate depends upward. `cctv-core` is the root of the graph and depends on no
sibling.

---

## 2. Application Interface Surface

All camera interactions pass through the typed `Facade` interface:

| Intent | Response | Facade entry point |
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

The full command vocabulary is enumerated in [03 — Command Catalogue](03-commands.md).

---

## 3. Crate Responsibilities & Contract Boundaries

| Crate | Responsibility | Contract boundary & dependencies |
| --- | --- | --- |
| `cctv-core` | The immutable device model: `CameraHandle`, `CommandId`, `CameraState`, `CameraError`, quirks table, edge-caching primitives, mock backend. | Pure `alloc`-only contract (`thiserror`, `async-trait`; `serde` optional). Zero I/O, zero sockets, zero Tokio. `#![no_std]` + `alloc` compatible. |
| `cctv-transport` | Data-driven TLS policy, time-budgeted `FrameReader`, zero-allocation XML scanner, TCP/TLS socket helpers. | `thiserror` required; `tokio-rustls` / `reqwest` optional behind `net` / `http`. Rustls only — no native TLS. |
| `cctv-rtp` | Pure byte-slice RTP header parsing and H.264/H.265 access-unit reassembly (STAP-A, FU-A, AP, FU). | Zero dependencies. Pure byte-slice processing. |
| `cctv-rtsp` | Pure message framing, SDP track extraction, `$`-interleaved frame demuxing, and RFC 2617/7616 Digest auth state machine. | `base64`, `zeroize`. Independent of socket I/O; consumed by streaming loops. |
| `cctv-driver` | Vendor-neutral adapter seam: the `DriverAdapter` factory surface, `DriverRegistry` with 5-step ladder resolution, `FingerprintRule`, `RtspTemplate`, and web UI endpoint proposal analyser (`analyse_web_ui`, `DriverProposal`). `discovery` and `onvif` (both default) carry the SSDP `M-SEARCH` / WS-Discovery `Probe` sweeps and the ONVIF SOAP 1.2 Device/Media/PTZ client with skewed-clock WS-Security `UsernameToken` and Profile G edge recording search/playback client; `ezviz` bundles the Ezviz Open Platform adapter; `uvc` / `uvc-ptp` / `ddc` bundle the local-video seam. | Optional drivers compile behind features; the seam itself depends only on `cctv-core`. |
| `cctv-agent` | Deterministic fleet reconciler, graded remediation actions (`Hold`, `MarkOffline`, `TakeOutOfService`), `StreamMux` GOP caching, and `SnapshotProxy`. | `cctv-core`, `cctv-rtsp`. Command issuance via `CommandRelay`; NALU broadcast caching. |
| `cctv-detect` | Motion and object detection primitives: `Detection`, `Zone` (`point`, `rectangle`), `ZoneConfig` with clamped normalization. | Pure / `alloc`-only. No ML frameworks, no I/O. |
| `cctv-rag` | Deterministic retrieval layer: temporal incident ledger, spatial topology graph, BM25 index, and diagnostic playbooks, exposed as one `KnowledgeBaseIndex`. | `cctv-core`, `cctv-detect`. Runtime-free; bounded in-memory index; no external vector database. |
| `cctv-pair` | Device pairing ceremony and trust store: the AKE handshake, `TrustStore` persistence, pairing-URI parsing, and `zbase32` human-comparable fingerprints. | `cctv-core` (`serde-sync`). Certificate-secured AKE. |
| `cctv-sync` | Peer-state convergence: CRDT primitives (`LwwRegister`, `OrSet`) and the device/trust registry replicas derived from them. | `cctv-core`, `cctv-pair`. Runtime-free convergence; no coordination server. |
| `cctv-rs` | Aggregate facade: unified driver factory registry, lifecycle management, zone processing, and discovery entry points. | All workspace crates. Single unified interface (`Arc<dyn CameraBackend>`) for host applications. |

---

## 4. Protocol & Transport Matrix

| Channel | Wire protocol | Auth / security | Handler crate |
| --- | --- | --- | --- |
| Discovery | UDP multicast (`239.255.255.250:1900`, `:3702`) | Unauthenticated | `cctv-driver` (`discovery`) |
| RTSP streaming | TCP / UDP RTSP 1.0 + interleaved RTP | HTTP Digest / Basic (RFC 2617 / RFC 7616) | `cctv-rtsp`, `cctv-rtp` |
| ONVIF control | HTTP/1.1 SOAP 1.2 XML | WS-Security `UsernameToken` (clock-skew compensated) | `cctv-driver` (`onvif`) |
| Cloud services | HTTPS REST JSON | OAuth / account access tokens | `cctv-driver` (`ezviz`) |
| Local video | USB / V4L2 / AVFoundation via OS seam | OS permissions | `cctv-driver` (`uvc`) |
| Encrypted transport | QUIC (`iroh`) | TLS 1.3 | `cctv-transport` (see [05](05-quic-tuning.md)) |

---

## 5. Architectural Invariants

1. **Forbid unsafe.** Every crate compiles with `#![forbid(unsafe_code)]`. Memory
   safety is enforced by the compiler.
2. **Panic-free operation.** No `unwrap()`, `expect()`, or panicking macros in
   library paths. Gaps return `CameraError::Unsupported`.
3. **`alloc`-only core.** `cctv-core` links no Tokio and no sockets, and its
   contract avoids heap-heavy untyped JSON (`serde_json::Value`) so it stays
   usable in constrained environments. It builds under `--no-default-features`
   and on a bare-metal target.
4. **Decoupled runtimes.** `cctv-core` never links Tokio. Host applications
   choose their own execution environment; drivers bind an async runtime only
   when network I/O is enabled.
5. **Zeroized secrets.** Passwords and keys are held in
   `zeroize::Zeroizing<String>`, wiped on drop, and redacted from `Debug` logs.
6. **Data-driven TLS policy.** TLS policies are plain data (pinned certs, ciphers,
   ALPN rules) rather than runtime-toggled globals, allowing per-camera
   self-signed certificate overrides without recompilation.
7. **Facade boundary.** Host applications communicate through `Facade` and
   `cctv-core` types; vendor protocol structs never leak into business logic.
8. **Separation of inference.** `cctv-detect` models zones and results without ML
   runtime dependencies. Inference models run externally; detections are fed back
   into the facade.
9. **Explicit workspace manifest.** Member crates are listed explicitly in the
   root `Cargo.toml` in dependency/publish order. No wildcard globs
   (`crates/*`) — a glob that stops matching fails the whole build opaquely.

---

## See also

- [02 — Engineering Standard](02-rust-package-standard.md) — the rules these crates are held to.
- [03 — Command Catalogue](03-commands.md) and [04 — Camera Taxonomy](04-camera-types.md) — the domain model referenced above.
- [05 — QUIC Tuning](05-quic-tuning.md) — the transport profile for the encryption channel.
- [07 — Execution Plan](07-plan.md) — how this topology evolved.

---

## License & Security

- **License:** Apache-2.0 ([LICENSE](../LICENSE)).
- **Security:** see [SECURITY.md](../SECURITY.md) for the vulnerability disclosure
  process.
