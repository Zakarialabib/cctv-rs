# 06 — Release Roadmap

> Milestone phases, release history, and the architectural decisions that are
> locked. **This is the authority for *what has shipped*;** forward scope lives in
> [07 — Execution Plan](07-plan.md).

**Read this when** you need the phase history or want to know which decisions are
no longer open for debate.

---

## 1. Phase Overview

| Phase | Milestone scope | Verification criteria | Status |
| --- | --- | --- | --- |
| 1 | `cctv-core`: alloc-only device model, error taxonomy, state machine, mock backend. | `cargo check -p cctv-core --no-default-features` clean, zero Tokio dependencies. | Completed |
| 2 | `cctv-transport`, `cctv-rtp`, `cctv-rtsp`: pure codecs, TLS policy, frame reading. | Workspace unit tests pass; RTP/RTSP codecs verified against RFC test vectors. | Completed |
| 3 | `cctv-driver` (`onvif` feature): SOAP builders/parsers, skewed-clock WS-Security `UsernameToken`. | Conformance tests pass against mock SOAP server fixtures. | Completed |
| 4 | `cctv-driver` (`ezviz` feature): Ezviz Open Platform cloud integration, token manager, PTZ mapping. | Unit tests pass against a mock HTTP server fixture. | Completed |
| 5 | `cctv-driver` (`uvc` / `uvc-ptp` / `ddc` features): local-video ports-and-adapters seam, UVC, PTP, HAL hot-plug, DDC/CI display control. | Unit tests pass across platform mocks. | Completed |
| 6 | Driver consolidation: `cctv-discovery`, `cctv-ezviz`, `cctv-stream-uvc` and `cctv-onvif` fold into `cctv-driver` behind features; `DriverAdapter` / `CameraBackend` split. | `cargo check -p cctv-driver --features ezviz,uvc,uvc-ptp,ddc` clean; workspace tests pass. | Completed |
| 7 | `cctv-agent`: fleet reconciler with graded remediation, `StreamMux`, `SnapshotProxy`. | Reconciler and stream multiplexer test suites pass. | Completed |
| 8 | `cctv-detect`, `cctv-pair`, `cctv-sync` & `cctv-rs`: detection device model, pairing/trust, CRDT convergence, facade registry, universal dispatch. | Full workspace tests and facade integration suite pass. | Completed |
| 9 | Host application integration: production deployment in edge video management clients. | Integration verification in an edge host application. | In progress |

---

## 2. Release History

### 0.4.0 — Fleet Operations & Edge Storage (2026-11-08)

Delivered on top of the 0.3.0 surface:

- **Edge storage caching primitives** (`cctv-core`): `EdgeCache`, a runtime-free,
  `alloc`-only bounded ring-buffer for recent snapshots and clips per camera, with
  deterministic oldest-first eviction.
- **Advanced fleet reconciliation policies** (`cctv-agent`): configurable
  hysteresis and staged rollout capping.
- The vendor-adapter consolidation (phase 6) completed: `cctv-onvif` was removed
  from the workspace, leaving the adapters as features of `cctv-driver`.
- **Driver Resolution Ladder & Fingerprinting** (`cctv-driver`): `FingerprintRule`,
  `RtspTemplate`, and priority bands resolving camera drivers via a 5-step ladder.
- **Reviewable Driver Proposals** (`cctv-driver`): `WebUiAnalyser` extracting RTSP/HTTP
  endpoints and generating human-reviewable `DriverProposal` PR templates.
- **ONVIF Profile G Client** (`cctv-driver::onvif`): edge recording search, playback URI,
  and summary retrieval for camera SD cards.

### 0.3.0 — Driver Expansion & Streaming Hardening (2026-11-02)

Eleven crates published to crates.io (`cctv-core`, `cctv-transport`, `cctv-rtp`,
`cctv-rtsp`, `cctv-driver`, `cctv-agent`, `cctv-detect`, `cctv-rag`, `cctv-pair`,
`cctv-sync`, `cctv-rs`) plus `ezviz-openapi` (own repository, consumed by
`cctv-driver`'s `ezviz` feature).

W6 delivered:

- **RTSP session reconnection hardening** — bounded exponential backoff
  (`RECONNECT_MAX_DELAY` 10 s, `RECONNECT_MAX_RETRIES` 8), `StreamEnd::Stalled`
  for a silent session, `StreamEnd::Failed` for a hard failure;
  `cctv_rtsp::streaming::run` keeps the media channel alive while the loop
  re-arms.
- **Live stream health watchdog** — the `cctv_core::health` state machine
  (`HealthWatchdog`, healthy / stalled / dead, alloc-only) wired into the driver
  stream path through `READ_STALL` and the reconnect policy.

Quirks fixed along the way: the request-response digest retry, the interleaved
media channel survival, and the stale-frame drop path in `FrameReader`.

Since 0.3.0 the vendor adapters consolidated into `cctv-driver` features
(`discovery` and `onvif`, both default; `ezviz`, `uvc`, `uvc-ptp`, `ddc`),
reducing the published surface from fifteen crates to eleven.

---

## 3. Locked Decisions

1. **Pure-Rust workspace.** No C-FFI, no vendor binary blobs. All crates are
   licensed under Apache-2.0.
2. **Explicit manifest.** Workspace members are declared in `Cargo.toml` in
   dependency publish order.
3. **Decoupled drivers.** Protocols reside in standalone crates; drivers do not
   pollute `cctv-core` with vendor concepts.
4. **`alloc`-only contract.** `cctv-core` has zero runtime or socket
   dependencies.
5. **No native TLS.** Network transport relies strictly on `rustls` for
   deterministic cross-platform behaviour.
6. **Health as a first-class contract.** A live stream is a health state, not a
   boolean (`cctv_core::health`, `cctv_rtsp::streaming` reconnect policy).

---

## See also

- [07 — Execution Plan](07-plan.md) — work packages, the consolidation ledger, and forward scope.
- [01 — Architecture](01-architecture.md) — the topology these phases produced.

---

## License & Security

- **License:** Apache-2.0 ([LICENSE](../LICENSE)).
- **Security:** see [SECURITY.md](../SECURITY.md) for the vulnerability disclosure
  process.
