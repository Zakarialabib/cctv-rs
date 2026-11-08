# 02 — Rust Workspace Engineering Standard

> The authoritative rules, invariants, and gates for every crate in this
> workspace. **This is the authority for process** — the other documents describe
> what the workspace *is*; this one describes how it is *held*.

**Read this when** you are adding a crate, changing a public error, touching
secrets, or opening a pull request. The pull request gate in
[`PULL_REQUEST_TEMPLATE.md`](../.github/PULL_REQUEST_TEMPLATE.md) mirrors §4.

---

## 1. Workspace Principles

- **Explicit members.** Member crates are listed explicitly in the root
  `Cargo.toml` in dependency and publish order. Wildcard path patterns
  (`crates/*`) are prohibited — a glob that stops matching fails the whole build
  with an opaque error.
- **Facade seam.** External applications integrate through `cctv-rs`. Internal
  driver crates never leak vendor-specific structs into application models.
- **Safety.** `#![forbid(unsafe_code)]` is enforced at the root of every crate.
- **Documentation.** Missing docs are denied (`missing_docs = "deny"` in the
  workspace lints), so the public surface cannot rot silently.

---

## 2. Error Contracts & Panic Policy

- **No panics in libraries.** `unwrap()`, `expect()`, `panic!()`,
  `unimplemented!()`, and `todo!()` are forbidden in production paths.
- **Domain errors.** Each crate defines a typed error enum via `thiserror`.
- **Capability handshake.** Unsupported hardware capabilities return
  `CameraError::Unsupported` rather than panicking or failing silently.

---

## 3. Security & Dependency Rules

- **Rustls only.** Native TLS / OpenSSL bindings are rejected; only pure-Rust TLS
  (`rustls`) is permitted in transport sockets. CI asserts nothing links
  `native-tls`.
- **`alloc`-only core.** `cctv-core` must remain free of Tokio and async socket
  runtimes (`cargo check -p cctv-core --no-default-features`).
- **Secret zeroization.** Passwords and keys are protected in memory with
  `zeroize::Zeroizing<String>` and redacted `Debug` implementations.
- **Minimal dependency surface.** A dependency is added only when it removes more
  code than it costs to audit. Optional protocol support lives behind cargo
  features, not in the default build.

---

## 4. Verification Gates

All checks must pass before pushing or releasing:

```bash
cargo fmt --all --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

CI adds jobs beyond this local set:

- **MSRV build** — `1.89.0` (pinned in `rust-toolchain.toml`).
- **Per-crate build** in publish order.
- **`cctv-core` graph assertions** — tokio-free and std-free, plus bare-metal
  (`thumbv7em-none-eabi`) compilation.
- **`cargo-deny`** — license, advisory, and ban checks.
- **Annex-B parser fuzz target** (`cctv-rtp`).

---

## See also

- [01 — Architecture](01-architecture.md) — the invariants §3 enforces, in context.
- [07 — Execution Plan](07-plan.md) §5 — the same gate set, as run by the plan.

---

## License & Security

- **License:** Apache-2.0 ([LICENSE](../LICENSE)).
- **Security:** see [SECURITY.md](../SECURITY.md) for the vulnerability disclosure
  process.
