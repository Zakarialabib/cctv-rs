# cctv-rs

A pure-Rust CCTV backend for SignageMaster: discover, stream, control (PTZ), analyse
(zones / motion / AI) and reconcile camera fleets — no Tauri, no UI, no SQLite.

It is the camera half of the architecture refactor: `src-tauri` hands the app its
typed facade, and each protocol lives in its own driver crate — `cctv-ezviz` (the
reverse-engineered Ezviz Open Platform client, `ezviz-openapi-rs`) and
`cctv-onvif` (ONVIF Device / Media / PTZ).

## Workspace

| Crate | Role |
|---|---|
| `cctv-core` | device model: types, state, commands, errors, mock, conformance (`no tokio`) |
| `cctv-transport` | TLS policy, pinning, budgeted frame reader, shared XML scanner and HTTP client |
| `cctv-discovery` | SSDP / multicast / USB camera discovery |
| `cctv-onvif` | ONVIF (Device / Media / PTZ) driver: SOAP builders + parsers |
| `cctv-ezviz` | Ezviz Open-Platform driver (from `ezviz-openapi-rs`) |
| `cctv-agent` | fleet reconciler: offline→armed→streaming, audit trail |
| `cctv-rs` | facade: discover, frame, ptz, start/stop, stats, config, presets |

The seven crates are versioned and published together. `cctv-core` depends on nothing
in the workspace, the facade depends on everything, and only `cctv-rs` is meant to
be consumed by the application.

## Quick start

```bash
git clone https://github.com/Zakarialabib/cctv-rs
cd cctv-rs
cargo check --workspace
cargo test --workspace
```

## What it is NOT

- Not a Tauri plugin. Nothing here owns a window or a SQLite DB.
- No vendor shipped inside the workspace: `ezviz-openapi-rs` is consumed as a
  dependency. PTZ protocols (ONVIF/RTSP) reach the app through the driver crates.
- Not a media server: `cctv-transport` only reads and buffers frames.

## Documentation

Start at [`docs/README.md`](docs/README.md). The architecture — use surface, crate
map, protocol matrix, event taxonomy and the integration seam — is
[`docs/architecture.md`](docs/architecture.md); the command inventory is
[`docs/commands.md`](docs/commands.md) and the plan is
[`docs/roadmap.md`](docs/roadmap.md). The house engineering standard is
[`docs/04-rust-package-standard.md`](docs/04-rust-package-standard.md).

## Contributing

Prerequisites, the local gates CI enforces, commit conventions and the versioning
policy are in [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Releasing

The seven crates publish in dependency order via
[`scripts/publish.sh`](scripts/publish.sh) — crates.io yanks rather than deletes,
so the order and the dry run matter. The order itself lives in that script.

## Security

Report vulnerabilities privately — see [`SECURITY.md`](SECURITY.md). There is no
`unsafe` in this workspace; every crate root forbids it.

## License

MIT — see [`LICENSE`](LICENSE). Changes are recorded in [`CHANGELOG.md`](CHANGELOG.md).
