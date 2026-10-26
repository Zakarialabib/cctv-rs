# cctv-transport

The single place a TLS stack enters `cctv-rs`, together with the budgeted frame
reader that sits between a camera and the rest of the fleet.

TLS policy is data — pinned certificates, a minimum version, an ALPN preference —
so a deployment (VPS mirror, office player, kiosk) can swap it at runtime
without recompiling. Frame reads are time-budgeted so one slow camera can never
stall the others.

## You get

- `policy` — TLS configuration as data.
  - `Policy` — minimum TLS version, pinned certificates, HTTP/2 preference.
  - `Policy::strict_pinned` — a pinned, TLS 1.3-only policy.
  - `TlsVersion` — `V1_2` / `V1_3` minimum-version choice.
- `reader` — budgeted, protocol-agnostic frame reading.
  - `FrameReader` — `new`, `try_fill`, `set_budget`.
  - `Frame` — width, height and raw payload bytes off the wire.
- `xml` — the total, namespace-prefix-agnostic XML scanner the drivers parse
  device responses with.
- `http` (feature `http`) — the shared async HTTP client: **rustls only**,
  HTTP Digest and Basic negotiated from the 401 challenge.
  - `HttpClient`, `HttpConfig`, `HttpResponse`, `HttpError`.
  - `digest_authorization`, `parse_challenge`, `basic_authorization` — pure,
    tested against the RFC 2617 worked example.
- `net` (feature `net`) — the raw socket layer, and the reason `http` can
  eventually go: TCP, UDP and TLS without `reqwest`. Applies policy rather than
  hardcoding trust.
  - `connect_tcp`, `connect_tls`, `bind_udp` — a `Stream` that is either plain
    TCP or a rustls session, so one driver reads both the same way.
  - `TcpOptions` — `timeout`, `tls`, `policy`, `accept_invalid_certs`. A camera
    that ships a self-signed certificate must opt in to accept it — it is
    off by default.
  - `read_chunks` / `feed_chunks` — the byte reader that pushes chunk sizes into
    `FrameReader`, which stays pure and takes `now_ms` from the caller.
  - `NetError` / `ErrorKind` — one error type for transport and policy, so a
    reconnect loop decides *retry or give up* without knowing which library
    failed.
- `fixture` (feature `fixture`) — a canned-response HTTP server (optionally
  demanding Digest auth) the driver conformance tests run against instead of a
  camera.

## Deliberately not

- `reqwest` when the `net` feature is on: zero HTTP client, and the crate stays
  usable anywhere tokio runs.
- No `unsafe`; MSRV 1.89, edition 2021.

See `docs/architecture.md` for the design.
