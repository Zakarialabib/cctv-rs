# cctv-rtsp

The protocol half of the `cctv-rs` stream path: RTSP framing, SDP, RTP headers,
and H.264 access-unit reassembly.

It is **pure** — byte slices in, no socket, no runtime, no TLS — because that is
where the bugs are. An RTSP session is a handful of requests; the frames are the
hard part: one frame may arrive as a single NAL unit, as a STAP-A aggregate of
several, or split across many FU-A fragments, and the only thing marking the end
is the RTP marker bit.

## Layout

| Module | What it holds |
| --- | --- |
| `message` | RTSP request / response framing, and the `$`-prefixed interleaved media frames a TCP transport multiplexes onto the same connection |
| `sdp` | The `DESCRIBE` reply: tracks, payload type, codec, `a=control`, and the out-of-band SPS/PPS |
| `rtp` | The 12-byte header and its payload, with CSRC lists, extensions and padding skipped |
| `h264` | Reassembly of single NALs, STAP-A and FU-A into access units |
| `error` | `RtspError`, including `Incomplete` |

## Parsing is incremental

`Message::parse` returns the message **and how many bytes it consumed**, so a
caller keeps the remainder for the next read. `RtspError::Incomplete` is not a
failure — it means "read more and try again", which is what makes the parser safe
on a socket where a TCP read lands mid-header:

```rust,no_run
use cctv_rtsp::{Message, RtspError};

fn feed(buf: &[u8]) -> Option<usize> {
    match Message::parse(buf) {
        Ok((_message, used)) => Some(used),
        Err(RtspError::Incomplete) => None,   // read more
        Err(error) => panic!("{error}"),
    }
}
```

## Two loss modes, handled explicitly

Both are normal on a real network, and both are *dropped* rather than emitted —
a decoder handed half a frame produces garbage, which is worse than a dropped
frame:

- **A lost fragment.** The FU-A start arrived, the end never did, and the marker
  closed the unit anyway.
- **A timestamp change mid-unit.** The next frame started while the previous one
  was still open. This is how loss is usually noticed *before* the marker.

`H264Depacketizer::dropped()` counts them, so a caller can tell a bad network
from a bad camera.

## Testing

`cargo test -p cctv-rtsp` — 40 tests, no camera, no server, no network. They pin
the things that actually break: a truncated buffer is `Incomplete` and not
`Malformed`; padding that claims more bytes than the packet holds is rejected
rather than slicing out of range; an `a=rtpmap` for payload type 97 is not
applied to a track that named 96; a FU-A header is rebuilt from the FU indicator's
NRI plus the FU header's type; a truncated STAP-A yields no unit rather than a
partial one.

## What this crate does not do

No socket loop, no RTSP Digest auth, no RTCP, no H.265/HEVC, no
`packetization-mode=2` (the interleaved STAP-B / MTAP / FU-B modes). Those are
unimplemented, not faked — the socket loop belongs in a driver, which is where the
TLS policy and the session state live.
