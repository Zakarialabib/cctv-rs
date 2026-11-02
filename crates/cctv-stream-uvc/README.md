# cctv-stream-uvc

[![Crates.io](https://img.shields.io/crates/v/cctv-stream-uvc.svg)](https://crates.io/crates/cctv-stream-uvc)
[![docs.rs](https://docs.rs/cctv-stream-uvc/badge.svg)](https://docs.rs/cctv-stream-uvc)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Local video capture and OS abstraction layer for `cctv-rs`. Implements a Ports & Adapters architecture isolating host OS device capture from library logic.

## Primitives

- **OS Platform Seam (`uvc`)**: `UvcAdapter` operating against an `OsPlatform` / `OsError` boundary, enabling full test coverage without attached physical hardware.
- **Budgeted Reader (`backpressure`)**: `BudgetedFrameReader` providing domain-driven backpressure that buffers `FrameItem` units cleanly.
- **PTP Capture (`ptp`)**: `PtpAdapter` / `PtpStatus` primitives providing synchronized capture-and-fetch workflows.
- **Hot-Plug Notifications (`hal`)**: `HalEventSource` abstractions delivering proactive peripheral arrival and removal events without polling loops.

## Invariants

- `#![forbid(unsafe_code)]`
- Zero internal socket connections; platform boundary belongs to caller.

## Usage

```rust,no_run
use cctv_stream_uvc::backpressure::BudgetedFrameReader;

let mut reader = BudgetedFrameReader::new(10);
assert_eq!(reader.len(), 0);
```
