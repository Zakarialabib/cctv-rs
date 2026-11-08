# Pull Request Template

## What does this PR do?

<!-- Brief description of changes -->

## Which crate(s) does this touch?

- [ ] cctv-core
- [ ] cctv-transport
- [ ] cctv-rtp
- [ ] cctv-rtsp
- [ ] cctv-driver

- [ ] cctv-agent
- [ ] cctv-detect
- [ ] cctv-rag
- [ ] cctv-pair
- [ ] cctv-sync
- [ ] cctv-rs
- [ ] workspace / tooling (`scripts/`, `xtask/`, `.github/`, docs)

## Does this break the build?

- [ ] No — workspace `cargo check --workspace` passes
- [ ] Yes — sub-task of the PR (explain below)

## Verification

- [ ] `cargo fmt --all --check` — clean
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` — 0 errors
- [ ] `cargo test --workspace` — all tests green
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` — clean

## Notes

<!-- Anything the reviewer should know? Device-validation pending, a new
     publish-order entry, a `deny.toml` change, etc. -->
