# Rust Package Engineering Standard

An authoritative standard for building and maintaining Rust library packages and workspaces within this repository.

---

## 1. Repository Layout & Workspace Principles

- **Explicit Workspace Members:** Member crates are listed explicitly in root `Cargo.toml`.
- **Facade Pattern:** Aggregates child crates behind one public interface without leaking vendor implementation details.
- **Crate Root Invariants:** Every crate root enforces `#![forbid(unsafe_code)]` and inherits `#![deny(missing_docs)]` via workspace lints.

---

## 2. Error Contract & Panic Policy

- **No Library Panics:** `unwrap()`, `expect()`, `panic!()`, `unimplemented!()`, and `todo!()` are forbidden on reachable library paths.
- **Typed Error Enums:** Every crate provides a `<Crate>Error` enum implemented via `thiserror`.
- **Explicit Capability Errors:** Unsupported device features return `CameraError::Unsupported` rather than panicking or failing silently.

---

## 3. Dependency & CI Policy

- **Workspace Dependency Declarations:** External dependencies and versions are declared centrally in `[workspace.dependencies]`.
- **Verification Gates:**
  - `cargo check --workspace --all-targets`
  - `cargo test --workspace`
  - `cargo clippy --workspace --all-targets -- -D warnings`
