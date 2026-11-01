# cctv-stream-uvc

[![Crates.io](https://img.shields.io/crates/v/cctv-stream-uvc.svg)](https://crates.io/crates/cctv-stream-uvc)
[![docs.rs](https://docs.rs/cctv-stream-uvc/badge.svg)](https://docs.rs/cctv-stream-uvc)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

UVC and PTP adapters plus the OS seam for `cctv-rs`. The Ports & Adapters layer for local video: frame acquisition behind a platform seam, domain-driven backpressure, PTP "Capture & Fetch" surfaces, and HAL hot-plug event sources.

## Capabilities

- **UVC streaming (`uvc`):** `UvcAdapter` behind the `OsPlatform` / `OsError` seam — frame acquisition isolated from the OS so the adapter is testable without hardware.
- **Backpressure (`backpressure`):** `BudgetedFrameReader` — domain-driven, budgeted reading that yields `FrameItem`s instead of flooding the consumer.
- **PTP (`ptp`):** `PtpAdapter` / `PtpStatus` — the "Capture & Fetch" surfaces for synchronised capture.
- **HAL events (`hal`):** `HalEventSource` — hot-plug event sources so device arrival and removal are observed, not polled.

All four modules are re-exported from the `cctv-rs` facade.

## Technical Guarantees

- `#![forbid(unsafe_code)]`
- No network sockets of its own — the seam is the boundary; wiring belongs to the caller.

## License

MIT
