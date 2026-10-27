# cctv-core

[![Crates.io](https://img.shields.io/crates/v/cctv-core.svg)](https://crates.io/crates/cctv-core)
[![docs.rs](https://docs.rs/cctv-core/badge.svg)](https://docs.rs/cctv-core)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

The foundational device model for `cctv-rs`. `cctv-core` defines the zero-runtime types, command catalogue, state machine, error taxonomy, and mock backend used across the entire workspace.

Designed for `alloc`-only targets (`#![no_std]` + `extern crate alloc`), `cctv-core` never pulls Tokio, async socket layers, or protocol dependencies into its graph.

## Core Capabilities

- **`CameraHandle` & `StreamHandle`:** Opaque, lightweight identifiers for cameras and active media streams.
- **`CommandId` Catalogue:** Enumerable command inventory with strict deterministic ordering (`order()`) and capability checking (`supports()`).
- **`CommandResult` & `CommandResultPayload`:** Strongly typed result envelopes for all camera operations.
- **`CameraState` Machine:** `CameraDesired`, `CameraStateCurrent`, and `ProbeResult` data types tracking fleet reconciliation states.
- **`CameraError` Taxonomy:** Rich, typed error enum eliminating library panics and sentinel error values.
- **`MockBackend` & Conformance Suite:** Complete mock driver and test suite ensuring all drivers adhere strictly to contract expectations.

## Quirks Engine (Feature `quirks`)

Vendor device and firmware defects are handled declaratively via data tables rather than hardcoded logic.

```rust,no_run
use cctv_core::quirks::Quirks;

let quirks = Quirks::bundled().expect("bundled TOML quirks parse");
if let Some(action) = quirks.action_for("vendor_x", Some("MODEL-100"), Some("V1.0.2")) {
    // Apply firmware-specific fixup
}
```

## Technical Guarantees

- `#![forbid(unsafe_code)]`
- `alloc`-only compatible (`default-features = false`).
- Zero async runtime or socket dependencies in the graph.
