# cctv-rtsp

The **session** half of the `cctv-rs` stream path: how a camera is asked for a
stream, and what it answers with.

It is **pure** — byte slices in, no socket, no runtime, no TLS — so every rule
is unit-tested without a camera, a server or a runtime. The payload half (RTP
headers, H.264 / H.265 reassembly) is [`cctv-rtp`](../cctv-rtp), a separate
crate, because a payload parser has nothing to do with a session and deserves
to be testable on its own.

## Layout

| Module | What it holds |
| --- | --- |
| `message` | RTSP request / response framing, and the `$`-prefixed interleaved media frames a TCP transport multiplexes onto the same connection. |
| `sdp` | The `DESCRIBE` reply: tracks, payload type, codec, `a=control`, and the out-of-band SPS/PPS. |
| `error` | `RtspError`, including `Incomplete`. |

## Parsing is incremental

`Message::parse` returns the message **and how many bytes it consumed**, so a
caller keeps the remainder for the next read. `RtspError::Incomplete` is not a
failure — it means "read more and try again", which is what makes the parser
safe on a socket where a TCP read lands mid-header:

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

## Testing

`cargo test -p cctv-rtsp` — 19 tests, no camera, no server, no network. They pin
the things that actually break: a truncated buffer is `Incomplete` and not
`Malformed`; the remainder of the buffer is left for the next message; a request
round-trips through its own serialiser with no `Content-Length` when it has no
body; an `a=rtpmap` for payload type 97 is not applied to a track that named 96;
a malformed line in SDP is skipped rather than aborting the session.

## What this crate does not do

No socket, no RTSP Digest auth, no keepalive. The `CameraBackend` driver —
DESCRIBE → SETUP (interleaved) → PLAY over TCP — belongs here once
`cctv-transport` grows a socket layer; `cctv-transport::http::digest_authorization`
already computes the digest and takes the method and URI, so it is reusable as-is.
