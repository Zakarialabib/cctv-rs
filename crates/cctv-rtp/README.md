# cctv-rtp

[![Crates.io](https://img.shields.io/crates/v/cctv-rtp.svg)](https://crates.io/crates/cctv-rtp)
[![docs.rs](https://docs.rs/cctv-rtp/badge.svg)](https://docs.rs/cctv-rtp)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Zero-dependency RTP header parsing and H.264 / H.265 video access-unit depacketization into contiguous NAL units.

`cctv-rtp` is pure byte-slice manipulation: no sockets, no async runtime, no memory allocations during header parsing.

## Codec Depacketization Details

- **RTP Header Parsing:** Full 12-byte header parsing, CSRC lists, header extensions, and padding calculations.
- **H.264 (RFC 6184):** Handles Single NAL Unit packets, STAP-A aggregation packets, and FU-A fragmentation units.
- **H.265 / HEVC (RFC 7798):** Handles Single NAL Unit packets, AP aggregation packets, and FU fragmentation units.
- **Loss Detection & Recovery:** Drops corrupted fragments mid-unit (e.g., missing FU start/end or timestamp drift) and tracks drop metrics (`dropped()`) without emitting partial or malformed NAL units to decoders.

## Codec Translation Summary

| Feature | H.264 | H.265 / HEVC |
| --- | --- | --- |
| NAL Header | 1 byte | 2 bytes |
| Aggregation Packets | STAP-A (Type 24) | AP (Type 48) |
| Fragmentation Units | FU-A (Type 28) | FU (Type 49) |

## Usage Example

```rust,no_run
use cctv_rtp::h264::H264Depacketizer;
use cctv_rtp::rtp::RtpHeader;

let mut depacketizer = H264Depacketizer::new();
let packet = &[/* raw RTP bytes */];

if let Ok(header) = RtpHeader::parse(packet) {
    let payload = &packet[header.header_len..packet.len() - header.padding_len];
    if let Ok(Some(access_unit)) = depacketizer.push(&header, payload) {
        println!("Reassembled complete NAL unit of {} bytes", access_unit.data.len());
    }
}
```

## Technical Guarantees

- `#![forbid(unsafe_code)]`
- Zero external dependencies.
- Zero-copy header inspection over byte slices.
