# cctv-rtp

**The payload.** Standalone RTP header parsing and H.264 / H.265 depacketisation
into contiguous NAL units.

A camera does not send frames; it sends RTP packets. One frame arrives as a
single NAL unit, as an aggregation of several, or split across many fragments —
and the only thing marking the end is the RTP marker bit. This crate puts them
back together.

**No dependencies at all.** Byte slices in, no socket, no runtime, no codec
library. It knows nothing about RTSP or SDP — that is `cctv-rtsp` — which is why
it is tested exhaustively and reusable by any transport.

## Layout

| Module | What it holds |
| --- | --- |
| `rtp` | The 12-byte header, with CSRC lists, extensions and padding handled. |
| `h264` | Access units from single NALs, STAP-A and FU-A (RFC 6184). |
| `h265` | Access units from single NALs, AP and FU (RFC 7798). |
| `types` | `AccessUnit` — the shape both depacketisers produce. |
| `error` | `RtpError`, including `Incomplete`. |

## Two loss modes, dropped rather than emitted

Both are normal on a real network, and both are **dropped** — a decoder handed
half a frame produces garbage, which is worse than a dropped frame:

- **A lost fragment** — the FU start arrived, the end never did, and the marker
  closed the unit anyway.
- **A timestamp change mid-unit** — the next frame started while the previous
  one was still open. This is how loss is usually noticed *before* the marker.

`dropped()` counts them on both depacketisers, so a caller can tell a bad
network from a bad camera.

## Codec differences that actually matter

| | H.264 | H.265 |
| --- | --- | --- |
| NAL header | 1 byte | **2 bytes** |
| Aggregation | STAP-A (type 24), 1-byte header then length-prefixed NALs | AP (type 48), 2-byte header then length-prefixed NALs |
| Fragmentation | FU-A (type 28), 2-byte prefix | FU (type 49), **3-byte** prefix |
| Rebuild | `(indicator & 0xE0) \| type` | `(byte0 & 0x81) \| (type << 1)`, byte 1 carried unchanged |

Not reassembled, and deliberately not guessed at: H.264 types 25–27
(interleaved STAP-B / MTAP / FU-B modes) and H.265 type 50 (PACI) plus the
reserved ranges.

## Testing

`cargo test -p cctv-rtp` — 34 tests, no camera, no server, no network. They pin
what breaks: padding claiming more bytes than the packet holds is rejected
rather than slicing out of range; a truncated aggregate yields no unit rather
than a partial one; a FU header is rebuilt from the indicator plus the fragment
header; a continuation whose start was lost is counted and ignored; the reserved
types do not corrupt the unit in progress.
