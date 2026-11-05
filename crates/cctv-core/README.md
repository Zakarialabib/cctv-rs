# cctv-core

[![Crates.io](https://img.shields.io/crates/v/cctv-core.svg)](https://crates.io/crates/cctv-core)
[![docs.rs](https://docs.rs/cctv-core/badge.svg)](https://docs.rs/cctv-core)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)

Foundational device model for `cctv-rs`. Pure `alloc`-only abstractions: traits, typed command inventory, state machine, zeroized secrets, error taxonomy, and conformance suite.

Zero Tokio, zero socket dependencies, zero panics.

## Primitives

- **Handles & Types**: Opaque `CameraHandle` and `StreamHandle` IDs; coarse-to-fine `CameraType` hardware classification.
- **Commands**: 56-variant typed `CommandId` catalogue with deterministic `order()` and capability checks via `supports()`.
- **Backend Trait**: Async `CameraBackend` trait defining uniform driver behavior.
- **State Machine**: Reconciler states (`CameraDesired`, `CameraStateCurrent`, `ProbeResult`).
- **Secret Safety**: `DecryptedCameraAuth` wrapping passwords in `zeroize::Zeroizing<String>` with redacted `Debug`.
- **Quirks Engine**: Declarative TOML hardware/firmware workaround engine (feature `quirks`).
- **Mock & Conformance**: Thread-safe `MockBackend` and verification suite for driver testing.

## Invariants

- `#![forbid(unsafe_code)]`
- `#![no_std]` + `extern crate alloc` (`--no-default-features`)
- Serde gated behind optional `serde-sync` feature flag.

## Usage

```rust,no_run
use cctv_core::{CameraBackend, CameraError, CameraHandle, CommandId, MockBackend};

let mut mock = MockBackend::new("mock-01");
let handle = CameraHandle::new("mock-01");

assert!(mock.supports(CommandId::GetCameraFrame));
```
