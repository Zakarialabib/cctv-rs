# cctv-rs

Your camera fleet over one handle: discover, stream, control and reconcile the
cameras that feed SignageMaster — pure Rust, no Tauri, no SQLite, no vendor code
shipped in the tree.

This is the camera half of SignageMaster. The app (`src/core/cctv.ts`) talks to
us through a single typed facade and the `cctv-core` device model, and every
protocol lives in its own driver crate, so a new camera only adds one crate.

## What you get

| Crate | For |
| --- | --- |
| `cctv-core` | The device model: types, commands, state machine, error taxonomy. The only thing the app codes against. |
| `cctv-discovery` | SSDP + WS-Discovery sweeps behind one read-only surface. Empty on a dead network — never a fabricated device. |
| `cctv-transport` | TLS policy, the budgeted `FrameReader`, and — behind the `net` feature — a `reqwest`-free TCP/UDP socket layer. |
| `cctv-rtp` | RTP header parse and H.264/H.265 depacketisation into contiguous NAL units. Zero dependencies. |
| `cctv-rtsp` | Pure session codecs: RTSP framing, SDP, the `$`-prefixed RTP demux. |
| `cctv-onvif` | ONVIF Device/Media/PTZ over SOAP 1.2 with WS-Security, as a `CameraBackend`. |
| `cctv-ezviz` | The Ezviz Open Platform, wrapped from `ezviz-openapi-rs`. |
| `cctv-agent` | Fleet reconciler: desired vs observed, graded remediation, audit trail. |
| `cctv-rs` | The facade: registry, commands, discovery, stats, fleet reconciliation. |

Only `cctv-rs` is meant to be consumed; everything else is a driver.

## Quick start

```bash
git clone https://github.com/Zakarialabib/cctv-rs
cd cctv-rs
cargo check --workspace && cargo test --workspace
```

## What it is not

- Not a Tauri plugin — nothing owns a window or a SQLite DB.
- Not a media server — `cctv-transport` only reads and buffers frames.
- Not a vendor surface — no Ezviz or ONVIF name reaches the public facade.

## Docs

Start at `docs/README.md` — the architecture, the command inventory and the plan
are in `docs/architecture.md`, `docs/commands.md`, `docs/roadmap.md` and the house
standard `docs/04-rust-package-standard.md`.

## Contributing

Prerequisites, the local gates CI enforces, commit conventions and the versioning
policy are in `CONTRIBUTING.md`.

## Releasing

The nine crates publish in dependency order via `scripts/publish.sh` — the ten you
touch, and `cargo publish` yanks rather than deletes, so an idempotent dry run
before you push matters. The order lives in that script.

## Security

Report vulnerabilities privately — see `SECURITY.md`. There is no `unsafe` in this
workspace; every crate root forbids it.

## License

MIT — see `LICENSE`. Changes are recorded in `CHANGELOG.md`.
