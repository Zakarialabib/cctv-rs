# cctv-rtsp

[![Crates.io](https://img.shields.io/crates/v/cctv-rtsp.svg)](https://crates.io/crates/cctv-rtsp)
[![docs.rs](https://docs.rs/cctv-rtsp/badge.svg)](https://docs.rs/cctv-rtsp)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Pure session codecs for RTSP protocol messaging, SDP (Session Description Protocol) parsing, and interleaved binary stream multiplexing.

Operates purely on byte slices: no network sockets, no runtime dependencies, no side effects.

## Capabilities

- **Message Parsing & Serialization:** Incremental parser for RTSP requests, responses, and `$`-prefixed interleaved binary frames on TCP connections.
- **SDP Parser:** Extracts media tracks, control URLs (`a=control`), payload types, clock rates, and out-of-band SPS/PPS parameter sets (`sprop-parameter-sets`).
- **Incremental Streaming Parser:** Returns `RtspError::Incomplete` when receiving partial frames, allowing callers to buffer socket reads cleanly.

## Usage Example

```rust,no_run
use cctv_rtsp::{Message, RtspError};

fn parse_stream_buffer(buffer: &[u8]) {
    match Message::parse(buffer) {
        Ok((msg, consumed)) => {
            println!("Parsed RTSP message consuming {} bytes: {:?}", consumed, msg);
        }
        Err(RtspError::Incomplete) => {
            // Wait for additional socket data
        }
        Err(e) => {
            eprintln!("RTSP protocol error: {:?}", e);
        }
    }
}
```

## Technical Guarantees

- `#![forbid(unsafe_code)]`
- Pure computational layer: completely decoupled from network I/O.
