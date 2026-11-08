# 07 — Execution Plan

> Single source of truth for `cctv-rs` delivery: shipped work packages, the driver
> consolidation ledger, release milestones, and the gate each one is held to.
> **This is the authority for scope and schedule.**

**Read this when** you are picking up work, checking whether something has
shipped, or looking for the verification gate a change must pass. Release history
lives in [06 — Roadmap](06-roadmap.md).

---

## 1. Release Milestones

| Milestone | Scope | Status |
| --- | --- | --- |
| **0.2.0** | Alloc-only core contract, `net` socket layer, RTP/RTSP codecs, ONVIF driver, agent remediation loop, `StreamMux`, `SnapshotProxy`, facade detection surface. | Released |
| **0.3.0** | Driver expansion and streaming hardening: bounded reconnect backoff, live-stream health watchdog, plus `cctv-detect` / `cctv-rag` / `cctv-pair` / `cctv-sync` on the published surface. | Released |
| **post-0.3.0** | Driver consolidation — the vendor adapters fold into `cctv-driver` behind cargo features. | Complete |
| **0.4.0** | Fleet operations and edge storage: edge caching primitives, composite recording profiles, advanced fleet reconciliation policies. | Released (2026-11-08) |

---

## 2. Work Packages (shipped)

### W1 — `cctv-core`: alloc-only contract

Traits, state, error taxonomy, mock backend and conformance suite, with no
runtime, no protocol and no JSON DOM. Two `std`-only pieces are features rather
than part of the contract: `testing` (the mock needs `Mutex`) and `quirks` (the
TOML table).

- Gate: `cargo check -p cctv-core --no-default-features`.
- Gate: `cctv-core` has no `tokio` in its dependency graph.

### W2 — `cctv-transport`: socket and TLS layer

The `net` feature for pure `tokio` + `rustls` sockets, `FrameReader` stream
parsing, the XML scanner, and the `http` client the ONVIF and Ezviz adapters share.

- Gate: `cargo test -p cctv-transport --features net`.
- Gate: the `net` feature pulls in no `reqwest` — it exists to replace it.

### W3 — Codecs and the RTSP session

`cctv-rtp` (RTP header + H.264/H.265 depacketizer, pure) and `cctv-rtsp` (RTSP
framing, SDP track extraction, `$`-interleaved demuxing, RFC 2617/7616 Digest auth
state machine).

- Gate: `cargo test -p cctv-rtp -p cctv-rtsp`.

### W4 — `cctv-driver`: the adapter seam

`DriverAdapter` (factory: `driver_key`, `capabilities`, `probe`, `attach`) split
from `CameraBackend` (the live handle), plus `DriverRegistry` / `InMemoryRegistry`
dispatch and the bundled adapters behind features.

- Gate: `cargo test -p cctv-driver`.
- Gate: `cargo check -p cctv-driver --features ezviz,uvc,uvc-ptp,ddc`.

### W5 — `cctv-agent`: reconciliation and streaming

The reconciler issues authorised remediation through `CommandRelay`; `StreamMux`
multiplexes GOP-cached NALUs; `SnapshotProxy` fronts frame reads. The graded policy
layer (`Policy`, `PolicyTracker`, `PolicyVerdict`) gates every action and writes an
audit entry.

- Gate: `cargo test -p cctv-agent`.

### W6 — Streaming hardening (0.3.0)

- RTSP session reconnection: bounded exponential backoff (`RECONNECT_MAX_DELAY`
  10 s, `RECONNECT_MAX_RETRIES` 8); `cctv_rtsp::streaming::run` keeps the media
  channel alive while the loop re-arms.
- Live-stream health: `cctv_core::health` (`Health`, `HealthChanged`,
  `HealthWatchdog`, alloc-only) driven by `READ_STALL` (15 s) and wired into the
  driver stream path. The loop reports `StreamEnd::Stalled` / `::Failed` /
  `::Retrying` / `::Cancelled`.

- Gate: the reconnect constants, the `cctv-core` health module, and the
  `rtsp_fixture` end-to-end suite.

### W7 — Fleet surface

`cctv-detect` (detection and zone models), `cctv-rag` (incident ledger, spatial
topology, BM25 retrieval), `cctv-pair` (pairing ceremony, trust store, zbase32
fingerprints), `cctv-sync` (CRDT convergence: LWW register, OR-set), and the
`cctv-rs` facade that re-exports all of them.

- Gate: `cargo test --workspace`.

---

## 3. Driver Consolidation Ledger (post-0.3.0)

The published surface shrank from fifteen crates to eleven: vendor adapters live
behind cargo features on `cctv-driver` instead of as standalone crates.

| Item | PR | Change |
| --- | --- | --- |
| CV-10 | #56 | `DriverAdapter` / `CameraBackend` split + registry dispatch |
| CV-14 | #57 | `CameraBackend::observe` and `Facade::observe` |
| CV-13 | #58 | `cctv-discovery` folds in behind the `discovery` feature |
| CV-12 | #59 | `cctv-ezviz` and `cctv-stream-uvc` fold in behind `ezviz`, `uvc`, `uvc-ptp`, `ddc` |
| CV-20 | #60 | Graded reconciliation policies with an audit trail |

`cctv-driver` now carries the whole adapter surface:

| Feature | Default | Provides |
| --- | --- | --- |
| `discovery` | yes | Merged SSDP + WS-Discovery `Probe` sweep, the `Datagram` / `Discoverable` seam and `UdpTransport` |
| `onvif` | yes | ONVIF SOAP services (Device / Media / PTZ), session, WS-Security; implies `discovery` (the ONVIF sweep filters the shared one) |
| `driver-mock` | no | In-repo fixture adapter for tests |
| `mobile` | no | `discovery` plus the tokio runtime the mobile probe needs |
| `ezviz` | no | Ezviz Open Platform cloud adapter |
| `pc-sdk` | no | `ezviz` with the vendor PC SDK |
| `uvc` | no | Local-video adapter: HAL hot-plug, budgeted frame reader |
| `uvc-ptp` | no | `uvc` plus the PTP still-image path |
| `ddc` | no | `uvc` plus DDC/CI display control |

`cctv-driver` sits in the `Cargo.toml` member order, the `scripts/publish.sh`
`ORDER`, and CI's per-crate job; `ezviz-openapi` remains an external crates.io
dependency pulled in only by the `ezviz` feature.

### Remaining guardrails

| Item | Change | Why |
| --- | --- | --- |
| ~~CV-16~~ | ~~Prove the `no_std` contract on a bare-metal target (`thumbv7em-none-eabi`) in the `core` CI job and extend the graph assertion~~ | ~~`--no-default-features` only proves the crate compiles without `std` on a `std` host; the bare-metal target proves nothing in the graph pulls `std` back in~~ (Shipped) |
| ~~CV-17~~ | ~~Exercise the feature matrix in CI: isolate every feature, `--no-default-features` per crate, `--all-features` for `cctv-core` / `cctv-transport` / `cctv-driver`, and both `ddc` facade paths~~ | ~~Only three non-default feature paths are built today, so a feature combination can rot unnoticed~~ (Shipped — the new `feature matrix` CI job runs it. It found the rot on its first run: `cctv-driver --no-default-features --features onvif` did not build, because `onvif::discovery` filters the shared sweep. `onvif` now implies `discovery`.) |

---

## 4. Host Application Integration

1. **Phase A (complete)**: host edge client adopts `cctv-rs` and stores
   `Facade::new().with_builtin_drivers()` in application state.
2. **Phase B (in progress)**: migrate network discovery sweeps and RTSP stream URI
   resolution to `cctv-rs`.
3. **Phase C**: connect application video consumers to `cctv-rs` stream readers
   and `StreamMux` feeds.
4. **Phase D**: migrate PTZ controls, presets, snapshots and detection zones to
   facade commands.
5. **Phase E**: deprecate and remove the legacy camera driver adapters.

---

## 5. Verification Gates

Every change passes these before it is pushed or released:

```bash
cargo fmt --all --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

CI adds the MSRV build (1.89.0), the per-crate build in publish order, the
feature matrix (every combination built on its own — see §3, CV-17), the
tokio-free and std-free `cctv-core` graph assertions, bare-metal
(`thumbv7em-none-eabi`) `cctv-core` compilation, `cargo-deny`, and the Annex-B
parser fuzz target. The rule set behind these gates is
[02 — Engineering Standard](02-rust-package-standard.md) §4.

---

## See also

- [06 — Roadmap](06-roadmap.md) — phase history and locked decisions.
- [01 — Architecture](01-architecture.md) — the topology the work packages built.
- [02 — Engineering Standard](02-rust-package-standard.md) — the authority for process.

---

## License & Security

- **License:** Apache-2.0 ([LICENSE](../LICENSE)).
- **Security:** see [SECURITY.md](../SECURITY.md) for the vulnerability disclosure
  process.
