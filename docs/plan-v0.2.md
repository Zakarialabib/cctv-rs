# Plan: v0.2 → v0.4

How the [target architecture](target-architecture.md) gets built, in what order,
and what proves each step. `roadmap.md` records what *is*; this records what
*will be*, with the gates that make it checkable rather than aspirational.

## How to read this

Every work package has a **gate**: a command or assertion that fails today and
passes when the work is done. If a package cannot state its gate, it is not a
work package — it is a wish. Nothing starts before the decisions below are
settled, because they move public APIs.

## Decisions

Resolved, with the reasoning, so the work does not relitigate them.

### D1 — `no_std` stops at `alloc`, and the JSON DOM leaves the contract

`cctv-core` becomes `#![no_std]` + `extern crate alloc`. What actually blocks
that today is not `std` — it is **`serde_json::Value` sitting inside the
contract**: `Preset::config`, `CameraConfig::extra`, `CommandResultPayload::Config`.

Nothing reads those fields. The drivers never touch `extra`; the presets are
`id`/`name`/`pan`/`tilt`/`zoom` and the `config` bag is never populated. So the
answer is not "gate serde_json behind a feature" — it is **delete the bags**:

- `Preset::config` → removed.
- `CameraConfig::extra` → removed. An unrecognised field in a stored config is
  not the contract's business.
- `CommandResultPayload::Config { config: Value }` → `Config { config: CameraConfig }`.
  The payload already had a typed struct available; it was serialising it to a DOM
  and handing the DOM over.

If a vendor genuinely needs a typed escape hatch later, it goes in that vendor's
crate, not in `cctv-core`. **This is the change that makes `alloc`-only real**,
and it is why D1 comes first.

### D2 — adopt `thiserror` and `async-trait`

`thiserror` replaces ~200 lines of hand-written `Display`/`Error` across
`cctv-core`'s error taxonomy and makes it declarative. `async-trait` replaces the
`BoxFuture<'_, Result<…>>` alias with ordinary `async fn` signatures in the
`CameraBackend` trait, which is what a driver author expects to write.

Both box allocations the code already paid for: `BoxFuture` boxes today, and
`async-trait` boxes the same way. The cost is mechanical — every `impl
CameraBackend` changes — which is precisely why it happens **before** the RTSP
and vendor drivers are written, not after.

**Verified, not assumed.** `thiserror 2.0.21` declares `default = ["std"]` and
`std = []`, so `default-features = false` gives the `core`-only build the contract
needs. `async-trait 0.1.92` declares no features at all and generates
`Pin<Box<dyn Future>>`, which needs `alloc` and nothing more. Both hold under
`alloc`-only, so the fallback (hand-rolled `Display` in core, `thiserror` in the
drivers) is not needed.

### D3 — `cctv-discovery` survives, and `cctv-onvif` implements `Discoverable` over it

The map folds WS-Discovery into `cctv-onvif`. That is right for WS-Discovery and
wrong for discovery as a whole: **SSDP `M-SEARCH` finds cameras that are not
ONVIF** — plenty of cheap devices answer it with a bare RTSP URL and nothing
else. Folding the sweep into `cctv-onvif` would force `cctv-rtsp` and every vendor
crate to depend on ONVIF in order to find anything on the LAN.

So: `cctv-discovery` keeps the codecs and the sweep; `cctv-onvif` implements
`Discoverable` by delegating to it and filtering for ONVIF-capable results. The
map's intent — "onvif handles WS-Discovery" — is satisfied at the trait, which is
the level a caller cares about.

### D4 — the registry keeps `Arc`, not `Box`

`Box<dyn CameraBackend>` cannot be shared, and three consumers need one camera at
once: the facade (commands), the reconciler (probes), and the application's
stream consumer (`Facade::frame_reader` already hands out an `Arc`). A `Box`
registry would have to serialise every command behind a lock to lend `&dyn`.

Recommendation: keep `Arc<dyn CameraBackend>` and amend the map. If `Box` is
wanted for a single-owner CLI someday, that is a different type in a different
crate, not a downgrade of the shared one.

### D5 — `cctv-ezviz` stays first-class

It is absent from the map but published, conformance-clean, and a cloud vendor
API — so it belongs in the **Proprietary** row beside hikvision/dahua/reolink,
not in a bucket. No folding.

### D6 — the socket layer lives in `cctv-transport`, behind a feature

RTSP needs a raw TCP stream; ONVIF needs to stop paying for `reqwest`. Both want
the same thing: `tokio` + `rustls` with the TLS policy that is already **data**
in `cctv-transport`. So one `net` feature there, and `reqwest` becomes optional
and eventually removable.

## Releases at a glance

| Release | Theme | Contains | Why here |
| --- | --- | --- | --- |
| **0.2.0** | *Real numbers* | W1 core contract · W2 socket layer · W3 RTSP backend | The RTSP driver is the gate on the app swap (see below), and W1 must precede it or the driver is written twice. |
| **0.3.0** | *The fleet* | W4 `Discoverable` · W5 agent acts · W6 reolink | Discovery + remediation is what makes it a fleet product rather than a camera library. |
| **0.4.0** | *The vendors* | W7 hikvision · W8 dahua · W9 facade polish | Each vendor is a protocol surface; none needs reverse engineering, all need a fixture. |

## Work packages

### W1 — `cctv-core`: the contract, alloc-only

**Goal.** `#![no_std]` + `alloc`, deps exactly `serde`, `thiserror`, `async-trait`.
D1's deletions, D2's adoption.

**Gate.**
```
cargo check -p cctv-core --no-default-features        # compiles without std
cargo tree -p cctv-core -e normal                     # serde, thiserror, async-trait, alloc-only
cargo test --workspace                                # conformance suite unchanged
```
The conformance suite is the regression gate: it is the one thing that must not
change behaviour while the contract is rewritten.

**Risk: high.** It touches every crate. That is why it is first.

### W2 — `cctv-transport`: the socket layer

**Goal.** `net` feature: TCP connect (plain and `rustls`), UDP bind, the existing
TLS policy applied to both, and a byte reader that feeds `FrameReader`.

**Gate.**
```
cargo test -p cctv-transport --features net           # TCP + TLS against the fixture server
cargo tree -p cctv-transport --features net -e normal # no reqwest
```
Self-signed certificates must work — cameras ship them — so the fixture test uses
one, and `accept_invalid_certs` is exercised rather than assumed.

### W3 — `cctv-rtsp`: `CameraBackend`

**Goal.** DESCRIBE → SETUP (interleaved) → PLAY over W2's socket, Digest auth via
`cctv-transport::http::digest_authorization` (it already takes the method and URI),
interleaved demux into `cctv-rtp`, keepalive, reconnect.

**Gate.**
```
cargo test -p cctv-rtsp                               # conformance 7/7 against a canned RTSP server
cargo test -p cctv-rs stats_are_measured              # fps > 0 through the facade
```
The second assertion is the milestone the whole release exists for: it is the
first time `camera_get_stats` reports a number it measured rather than a zero it
assumed.

### W4 — `cctv-onvif`: `Discoverable`

**Goal.** Implement `Discoverable` (D3), delegating the sweep to `cctv-discovery`;
move the HTTP calls onto W2's socket layer and drop `reqwest`.

**Gate.**
```
cargo test -p cctv-onvif                              # discovery finds the fixture device
cargo tree -p cctv-onvif -e normal                    # no reqwest
```

### W5 — `cctv-agent`: discover, compare, **act**

**Goal.** `Vec<CameraConfig>` → discovery → `DeviceState` comparison → issued
`ControlCommand`s, not just a grade. The current reconciler decides and records;
it never acts, which is half a reconciler.

**Gate.** A test where the agent discovers a drifted fixture device and the mock
records the command it issued — the assertion is on the *command*, not the grade.

### W6–W8 — vendors

`cctv-reolink` first: its API is the cleanest and fully documented. Then
`cctv-hikvision` (ISAPI), then `cctv-dahua` (CGI). Each gets the same treatment as
ONVIF and Ezviz — conformance suite plus a canned server, no camera, no
credentials, no network.

### W9 — `cctv-rs`: facade polish

The `Registry` gains attach-from-discovery; `camera_get_all_stats` stops probing
every camera synchronously; the driver registry ships `onvif` and `rtsp`.

## Dependency graph

```
W1 (core contract) ──┬──► W2 (socket) ──┬──► W3 (rtsp backend) ──► [app swap Phase C]
                     │                  ├──► W4 (onvif discoverable)
                     │                  └──► W6..W8 (vendors)
                     └──► W5 (agent acts) ── needs W4's Discoverable
W9 (facade) ── after W3, before the app swap completes
```

W1 blocks everything. W3 blocks the integration. W5 blocks nothing external and
can slip to 0.3 without cost.

## Simple-Signage integration

The app is `C:\laragon\www\Simple-Signage` (Tauri v2). Its camera surface is
**Rust**: `src-tauri/src/camera/{manager,stream,capture,discovery,runtime_stats,webrtc}.rs`.
It already pins `tv-rs-power = "0.2.0"`, so depending on a sibling `*-rs` crate is
an established pattern.

**Ordering constraint, stated once and load-bearing:** `runtime_stats` is fed by
`camera_get_stats`. Swapping before W3 lands regresses working numbers to zeros.
So the swap starts *after* W3's gate, not before.

| Phase | Change | Gate | Rollback |
| --- | --- | --- | --- |
| **A** | Add `cctv-rs` as a dependency; construct a `Facade` beside the in-house manager. Nothing swapped. | App builds; both paths compile; no behaviour change. | Revert one dependency. |
| **B** | Discovery and stream-URL resolution move to `cctv-rs`. Read-only, lowest risk. | The app's discovery finds the same cameras it did. | Config flag back to the in-house path. |
| **C** | `runtime_stats` reads `FrameReader`. | **fps and bitrate are non-zero on a real camera**, and match the old path within tolerance. | Flag. |
| **D** | PTZ, presets and capture move to `cctv-rs` commands. | The app's existing camera tests pass unchanged. | Flag. |
| **E** | Delete the in-house implementation, the flag, and the shim. | `grep` finds no reference to the old manager. | — (intentional point of no return) |

Phase E follows the house rule: deprecate fully, no interim state, no
backwards-compatibility shims left behind.

**The error seam.** The app has its own camera error type; `cctv-core` has
`CameraError`. One adapter in `camera/` maps between them until Phase E, at which
point `CameraError` becomes the app's — which is what the map implies, since the
facade is meant to *be* the camera surface.

## Risk register

| Risk | Why it bites | Mitigation |
| --- | --- | --- |
| W1 is a big-bang refactor | `no_std` + `thiserror` + `async-trait` in one pass touches every crate | The conformance suite is the gate; do it before the drivers exist so nothing is written twice |
| `async-trait` boxes every call | One allocation per command, where `BoxFuture` already boxed | Acceptable, and the hot path — frame delivery — never goes through the trait; it goes through `FrameReader` |
| Self-signed camera certificates | Cameras ship them; rustls rejects them by default | The TLS policy is already data; the W2 fixture uses a self-signed cert and asserts the opt-in works |
| RTSP transport varies by camera | Interleaved vs separate UDP ports | **0.2 does interleaved only**; UDP is a documented gap, not a silent failure |
| RTSP Digest differs from HTTP | Different method token, same algorithm | `digest_authorization` already takes the method and URI — reuse, do not reimplement |
| The app swap regresses stats | `runtime_stats` depends on `camera_get_stats` | The ordering constraint above; Phase C is gated on real numbers |
| 12 crates on crates.io | New-crate rate limit: burst of 5, then 1 per 10 min | `publish.sh` already waits it out and skips what is published |

## Non-goals

Stated so they are not quietly assumed:

- **WebRTC** stays in the application. It is a delivery mechanism, not a camera
  protocol, and `cctv-rs` does not need to own it.
- **No media server, no transcoding, no encoding.** The workspace reads streams.
- **H.265 encoding** — depacketisation only.
- **ONVIF events** (`PullPoint`) and the imaging / analytics services.
- **USB enumeration** — the application's, as `architecture.md` already says.
- **`no_std` for the drivers.** Only the contract is `alloc`-only; a driver has
  sockets and therefore `std`.
