# cctv-rtsp

[![Crates.io](https://img.shields.io/crates/v/cctv-rtsp.svg)](https://crates.io/crates/cctv-rtsp)
[![docs.rs](https://docs.rs/cctv-rtsp/badge.svg)](https://docs.rs/cctv-rtsp)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)

Pure session codecs for RTSP 1.0 messaging, SDP (Session Description Protocol) parsing, and interleaved stream demuxing.

Pure computational layer: decoupled from network sockets, operating strictly on byte buffers.

## Primitives

- **Framing & Codecs**: Incremental parser for RTSP requests, responses, and `$`-prefixed interleaved binary frames.
- **SDP Extraction**: Parses media tracks, control paths (`a=control`), payload formats, clock rates, and SPS/PPS (`sprop-parameter-sets`).
- **Digest Authentication**: RFC 2617 & RFC 7616 challenge parser for `401 WWW-Authenticate: Digest` (`qop="auth"` supported) and response generator with `zeroize` secret integration.
- **Incremental Parsing**: Emits `RtspError::Incomplete` on partial buffers for clean async socket accumulation.

## Invariants

- `#![forbid(unsafe_code)]`
- Pure byte-slice protocol codecs; network I/O remains isolated in client layers.

## Usage

```rust,no_run
use cctv_rtsp::{Message, RtspError};

fn process_chunk(buffer: &[u8]) {
    match Message::parse(buffer) {
        Ok((msg, consumed)) => {
            println!("Parsed RTSP message (consumed {} bytes): {:?}", consumed, msg);
        }
        Err(RtspError::Incomplete) => {
            // Buffer additional incoming socket bytes
        }
        Err(e) => eprintln!("RTSP protocol error: {:?}", e),
    }
}
```
