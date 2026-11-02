# cctv-rtp

[![Crates.io](https://img.shields.io/crates/v/cctv-rtp.svg)](https://crates.io/crates/cctv-rtp)
[![docs.rs](https://docs.rs/cctv-rtp/badge.svg)](https://docs.rs/cctv-rtp)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Zero-dependency RTP header parsing and H.264 / H.265 video access-unit depacketization into contiguous NAL units.

Pure byte-slice manipulation: zero sockets, zero runtime dependencies, zero heap allocations during header parsing.

## Primitives

- **RTP Parsing**: Full 12-byte header parsing, CSRC lists, header extensions, and padding calculations.
- **H.264 (RFC 6184)**: Reassembles Single NAL Units, STAP-A aggregation packets, and FU-A fragmentation units.
- **H.265 / HEVC (RFC 7798)**: Reassembles Single NAL Units, AP aggregation packets, and FU fragmentation units.
- **Loss Detection**: Drops corrupted mid-unit fragments without emitting broken NAL units; tracks drop statistics via `dropped()`.

## Invariants

- `#![forbid(unsafe_code)]`
- Zero external runtime dependencies.
- Zero-copy header inspection over raw byte slices.

## Usage

```rust,no_run
use cctv_rtp::h264::H264Depacketizer;
use cctv_rtp::rtp::RtpHeader;

let mut depacketizer = H264Depacketizer::new();
let packet: &[u8] = &[/* raw RTP bytes */];

if let Ok(header) = RtpHeader::parse(packet) {
    let payload = &packet[header.header_len..packet.len() - header.padding_len];
    if let Ok(Some(access_unit)) = depacketizer.push(&header, payload) {
        println!("Reassembled NAL unit: {} bytes", access_unit.data.len());
    }
}
```
