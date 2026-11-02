# Rust Workspace Engineering Standard

Authoritative standards, invariant rules, and CI gates for crates in this workspace.

---

## 1. Workspace Principles

- **Explicit Members**: Member crates are listed explicitly in root `Cargo.toml` in dependency and publish order. Wildcard path patterns (`crates/*`) are prohibited.
- **Facade Seam**: External applications integrate through `cctv-rs`. Internal driver crates never leak vendor-specific structs into application models.
- **Safety**: `#![forbid(unsafe_code)]` is enforced at the root of every crate.
- **Documentation**: Missing docs are denied (`missing_docs = "deny"` in workspace lints).

---

## 2. Error Contracts & Panic Policy

- **No Panics in Libraries**: `unwrap()`, `expect()`, `panic!()`, `unimplemented!()`, and `todo!()` are forbidden in production paths.
- **Domain Errors**: Each crate defines a typed error enum via `thiserror`.
- **Capability Handshake**: Unsupported hardware capabilities must return `CameraError::Unsupported` rather than panicking or failing silently.

---

## 3. Security & Dependency Rules

- **Rustls Only**: Native TLS / OpenSSL bindings are rejected. Only pure Rust TLS (`rustls`) is permitted in transport sockets.
- **Alloc-Only Core**: `cctv-core` must remain free of Tokio and async socket runtimes (`cargo check -p cctv-core --no-default-features`).
- **Secret Zeroization**: Passwords and keys must be protected in memory using `zeroize::Zeroizing<String>` with redacted `Debug` implementations.

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
