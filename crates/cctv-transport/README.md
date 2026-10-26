# cctv-transport

The single place a TLS stack enters `cctv-rs`, together with the budgeted frame
reader that sits between a camera and the rest of the fleet. TLS policy is data
— pinned certificates, a minimum version, an ALPN preference — so a deployment
(VPS mirror, office player, kiosk) can swap it at runtime without recompiling,
and frame reads are time-budgeted so one slow camera can never stall the others.

- `policy` — TLS configuration expressed as data.
  - `Policy` — minimum TLS version, pinned certificates, HTTP/2 preference.
  - `Policy::strict_pinned` — a pinned, TLS 1.3-only policy.
  - `TlsVersion` — the `V1_2` / `V1_3` minimum-version choice.
- `reader` — budgeted, protocol-agnostic frame reading.
  - `FrameReader` — `new`, `try_fill`, `set_budget`.
  - `Frame` — width, height and raw payload bytes off the wire.
- `xml` — the total, namespace-prefix-agnostic XML scanner the drivers parse
  device responses with: `elements`, `element_text`, `element_body`,
  `attribute`, `soap_fault`, `escape` / `unescape`.
- `http` (feature `http`) — the shared async HTTP client: `reqwest` with
  **rustls only**, HTTP Digest (MD5 / SHA-256, `qop=auth`) and Basic negotiated
  from the 401 challenge.
  - `HttpClient::request`, `HttpConfig`, `HttpResponse`, `HttpError`.
  - `digest_authorization`, `parse_challenge`, `basic_authorization` — pure,
    tested against the RFC 2617 worked example.
- `net` (feature `net`) — the raw socket layer, and the reason `http` can
  eventually go: TCP, UDP and TLS without `reqwest`. Applies `policy` rather
  than hardcoding trust.
  - `connect_tcp`, `connect_tls`, `bind_udp` — a `Stream` that is either plain
    TCP or a `rustls` session, so one driver reads both the same way.
  - `TcpOptions` — `timeout`, `tls` (`TlsMode::Default` / `AcceptAny`),
    `policy`, `accept_invalid_certs`. Cameras ship self-signed certificates, so
    accepting one is an explicit opt-in, off by default.
  - `read_chunks` / `feed_chunks` — the byte reader that pushes chunk sizes into
    `FrameReader`, which stays pure and takes `now_ms` from the caller.
  - `NetError` / `ErrorKind` — one error type for transport and policy, so a
    reconnect loop decides *retry or give up* without knowing which library
    failed.
- `fixture` (feature `fixture`) — `FixtureServer` / `Route`: a canned-response
  HTTP server (optionally demanding Digest auth) that the driver conformance
  tests run against instead of a live camera.

See [`docs/architecture.md`](../../docs/architecture.md) for the full
architecture.
