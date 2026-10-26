# Target architecture (v0.2 and beyond)

The shape the workspace is being driven toward, and — more usefully — the delta
between it and what exists today. Written before the work starts so the gaps are
decisions rather than surprises.

This document is **aspirational**: [`architecture.md`](architecture.md) describes
what is built. Where the two disagree, this one is the plan and that one is the
truth.

## The map

| Crate | Role | Contract |
| --- | --- | --- |
| `cctv-core` | **The Contract.** Traits, enums, structs, errors. | Pure `no_std` (or `alloc` only). Zero dependencies besides `serde`, `thiserror`, `async-trait`. |
| `cctv-transport` | **The Pipe.** Socket management, TLS pinning, budgeted frame readers, XML/JSON scanners. | Pure-Rust TCP/UDP. |
| `cctv-rtsp` | **The Standard.** `CameraBackend` for generic RTSP cameras. | SDP parsing, RTP interleaved over TCP or on separate UDP ports. |
| `cctv-rtp` | **The Payload.** Standalone RTP header parsing and H.264 / H.265 depacketisation into contiguous NAL units. | Depends only on `cctv-core`. |
| `cctv-onvif` | **The Enterprise.** `CameraBackend` **and** `Discoverable` over SOAP/HTTP. | Handles WS-Discovery multicast. |
| `cctv-hikvision` / `cctv-dahua` / `cctv-reolink` | **The Proprietary.** `CameraBackend` over vendor HTTP/JSON/XML or binary protocols. | One crate per vendor. |
| `cctv-agent` | **The Brain.** Async reconciler. | Takes `Vec<CameraConfig>` (desired), runs `Discoverable`, compares against `DeviceState`, issues `ControlCommand`s to remediate drift — re-authenticating, restarting streams. |
| `cctv-rs` | **The Facade.** Re-exports `cctv-core`, holds a `Registry` of `Box<dyn CameraBackend>`, routes commands to the right driver. | The only crate the application depends on. |

## The delta

Measured against `main` at `a4121b5` (v0.1.0 plus the frame reader and
`cctv-rtsp`'s codecs).

| Target | Built today | Gap |
| --- | --- | --- |
| `cctv-core`: `no_std` + `alloc`, deps `serde`/`thiserror`/`async-trait` | `std`, `serde` + `serde_json`; hand-rolled `Display`; a `BoxFuture` alias instead of `async-trait` | Three separate changes. **(a)** `serde_json::Value` sits *in the contract* — `Preset::config`, `CameraConfig::extra`, `CommandResultPayload::Config` — and that is what actually blocks `alloc`-only. **(b)** `thiserror` would replace ~200 lines of hand-written `Display`/`Error`. **(c)** `async-trait` would replace `BoxFuture`, which changes every `CameraBackend` method signature and every driver. |
| `cctv-transport`: pure-Rust TCP/UDP, TLS pinning | TLS policy as **data** (pinning, min version, ALPN) and the frame reader are real; the HTTP client is `reqwest` behind a feature | No socket layer of its own. `reqwest` is a large dependency for a workspace whose selling point is "pure Rust, no native-tls". A `tokio` + `rustls` socket module would drop it — and RTSP needs a raw TCP stream anyway, so it is on the critical path regardless. |
| `cctv-rtsp`: `CameraBackend` for generic RTSP | Pure codec only: RTSP framing, SDP, RTP header, H.264 reassembly. No socket, no backend | The driver: DESCRIBE → SETUP → PLAY, RTSP Digest auth (`cctv-transport::http::digest_authorization` already computes it and takes the method and URI), interleaved demux, keepalive, reconnect. **This is the last step to real `fps`.** |
| `cctv-rtp`: standalone, H.264 **and H.265**, depends only on `cctv-core` | `rtp.rs` and `h264.rs` live inside `cctv-rtsp`, which depends on `base64` | Extract into `cctv-rtp`; add HEVC (FU + AP + PACI). The `base64` dependency belongs to SDP's `sprop-parameter-sets`, which stays in `cctv-rtsp` — so `cctv-rtp` lands at **zero** dependencies, better than the target asks. |
| `cctv-onvif`: `CameraBackend` **and** `Discoverable`, WS-Discovery multicast | `CameraBackend` ✓ (conformance 7/7). WS-Discovery lives in `cctv-discovery`, not here | `cctv-discovery` is **absent from the target map**. Either it folds into `cctv-onvif` (WS-Discovery is ONVIF-specific, so this is defensible) or it stays as the shared transport and `cctv-onvif` delegates. Needs a decision — see below. |
| `cctv-hikvision` / `cctv-dahua` / `cctv-reolink` | Absent | Three new crates. ISAPI, Dahua CGI and the Reolink API are all publicly documented, so none needs reverse engineering — but each is a real protocol surface, and each needs a canned-server test like the ONVIF and Ezviz drivers have. |
| `cctv-agent`: `Vec<CameraConfig>` → discover → compare → **issue `ControlCommand`s** | Holds desired/observed `CameraState`, grades a remediation (`Grade`/`Action`), records it in an audit trail. **It grades; it does not act.** | Two gaps: it never runs a discovery pass, and it never issues a command. Today `Facade::reconcile` probes and grades — the acting is the caller's job. The target wants the agent to drive remediation itself. |
| `cctv-rs`: `Registry` of `Box<dyn CameraBackend>` | `Registry`-shaped map of `Arc<dyn CameraBackend>` | `Box` cannot be shared. The facade, the reconciler and the application's stream consumer all need the same backend concurrently, and `Facade::frame_reader` hands out an `Arc`. Either keep `Arc`, or the registry owns the boxes and lends `&dyn` under a lock — which serialises every command. **Recommend keeping `Arc`.** |

## Decisions — resolved

The questions this map raises are settled in [plan-v0.2.md](plan-v0.2.md), with
the reasoning, so the work does not relitigate them:

| # | Question | Answer |
| --- | --- | --- |
| D1 | How far does `no_std` go? | `alloc`-only — and the **JSON DOM leaves the contract**, which is what actually blocks it |
| D2 | Adopt `thiserror` / `async-trait`? | Yes, and **before** the drivers are written, not after |
| D3 | Does `cctv-discovery` survive? | Yes; `cctv-onvif` implements `Discoverable` over it — SSDP finds non-ONVIF cameras |
| D4 | `Box` or `Arc` in the registry? | `Arc` — three consumers need one camera at once |
| D5 | Where does `cctv-ezviz` sit? | Proprietary, first-class, not folded into a bucket |
| D6 | Where does the socket layer live? | `cctv-transport`, behind a `net` feature |

## The integration seam: Simple-Signage

The application is `C:\laragon\www\Simple-Signage` (Tauri v2). Its camera surface
is **Rust**, not TypeScript:

```
src-tauri/src/camera/
├── manager.rs        20 KB — the current in-house camera manager
├── stream.rs         RTSP validation and the stream manager
├── capture.rs        stills
├── discovery.rs      network sweep
├── runtime_stats.rs  the fps / bitrate store
├── webrtc.rs         the WebRTC path
└── protocol/
```

That is the surface `cctv-rs` is meant to back, and the app already consumes the
sibling workspace — `src-tauri/Cargo.toml` pins `tv-rs-power = "0.2.0"` — so the
pattern for depending on a `*-rs` crate from it is established, not invented.

The seam is therefore: `cctv-rs` replaces the internals of `camera/`, and
`manager.rs` becomes a thin adapter over `Facade`. `runtime_stats.rs` is what
`FrameReader`'s measurements feed. `webrtc.rs` is **not** in `cctv-rs`'s remit and
should keep its own path.

Two consequences worth planning for:

- **`camera_get_stats` must be real before the swap**, or `runtime_stats` regresses
  from working numbers to zeros. That makes the RTSP socket loop a prerequisite of
  the integration, not a nice-to-have.
- **The app's error type and `cctv-core`'s must be compatible at the seam.** Today
  `CameraError` is `cctv-core`'s own enum; the app has its own. The adapter maps
  one to the other in one place, or `cctv-core`'s becomes the app's — the second is
  cleaner and is what the target map implies.
