# Contributing

## Prerequisites

- Rust 1.89 or newer (`rust-version` in `Cargo.toml`; CI builds that exact version).
- `rustfmt` and `clippy` components installed — CI fails without them, and a
  host that lacks them silently produces a passing-looking local run.

```bash
rustup component add rustfmt clippy
```

## Gates

Branch protection requires four checks — rustfmt, clippy, tests on Linux, tests on
Windows. Run the same four commands before pushing:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

CI runs four more jobs that are not branch protection but are still red builds you
will be asked to fix:

| Job                        | What it catches that a local `--workspace` run does not                      |
| -------------------------- | ---------------------------------------------------------------------------- |
| `msrv (1.89.0)`            | a `rust-version` nobody builds against, and code that needs a newer compiler |
| `cargo-deny`               | advisories, unknown sources, wildcard deps, licences outside `deny.toml`     |
| `per-crate features`       | a crate that only compiles because a sibling turned a feature on             |
| `core (tokio-free + mock)` | tokio sneaking into `cctv-core`'s graph                                      |

The last one is the reason the `--workspace` runs are not enough: feature unification
hides exactly the break that job exists to find. `cctv-core` is the contract the app
embeds, so it must stay free of a runtime.

Camera tests against live hardware are opt-in and must never run in CI.

## Layout rules

Read [`docs/architecture.md`](docs/architecture.md) first; each crate has one
reason to exist.

- `cctv-core` is the device model. It must not gain a network stack, an async
  runtime or vendor vocabulary.
- A driver crate is protocol only. `cctv-ezviz` wraps `ezviz-openapi-rs` through a
  git dependency pinned to a rev (published crates use the crates.io release); it never copies vendor code and never reaches the app directly.
- The app talks to `cctv-rs` (the facade), and nothing else. Vendor names must not
  appear in the facade's public surface.
- Library code must never panic and must never use `unimplemented!()`. A capability
  a camera does not have is a typed `CameraError`.
- Commands are addressed by `CommandId` (the *what*); drivers answer the *how*.

## Commit convention

[Conventional Commits](https://www.conventionalcommits.org/), scoped by crate
where it helps, one logical change per commit:

```
feat(cctv-ezviz): map the device list into EzvizCamera
fix(cctv-core): record probe_at when applying a probe
docs: reconcile the architecture doc against the manifests
chore(lint): satisfy clippy -D warnings
```

Group related files in a commit; do not mix formatting churn with behaviour.

## Versioning policy

`0.x` until the command catalogue and the error taxonomy have been stable for a full
release cycle; from `1.0`, plain [SemVer](https://semver.org/spec/v2.0.0.html).

What counts as breaking while at `0.x`, and needs a minor bump (`0.1` → `0.2`) plus a
CHANGELOG entry under **Changed** or **Removed**:

- adding a variant to an enum a caller matches on without `_ =>` — `CameraError`,
  `CameraType`, `CommandId`, `CommandResultPayload`, `CameraStateCurrent`. Adding a
  variant is additive for the crate and breaking for the caller, and pretending
  otherwise is how a patch release breaks a build;
- removing or renaming a public item, or changing a method's argument order;
- changing a default that alters behaviour (a frame budget, a TLS minimum, a
  confirmation threshold);
- raising `rust-version`.

Not breaking: a new driver crate, a new command that is answered by an existing
driver, a new fixture in the conformance suite, a new field on a struct that is
`#[non_exhaustive]` or not constructed by callers.

**The error taxonomy and the command catalogue are the strictest surfaces.** Once
`0.2` ships, `CameraError` variants may not be removed or repurposed for at least 90
days, because remedy mapping lives in downstream retry code. Add a variant, add its
default result, add the conformance case — in the same PR.

All six crates are versioned together and published together by
[`scripts/publish.sh`](scripts/publish.sh). A partial release is worse than a late
one.

## Releasing

The crates publish in dependency order; crates.io yanks rather than deletes, so a
partial publish burns the version. Dry-run first:

```bash
./scripts/publish.sh --dry-run
./scripts/publish.sh --publish
```

`ezviz-openapi` is published from its own repository and must be on crates.io
before `cctv-ezviz`.

## Docs

Start at [`docs/README.md`](docs/README.md). The house engineering standard is
[`docs/04-rust-package-standard.md`](docs/04-rust-package-standard.md); it is the
authority for everything above.
