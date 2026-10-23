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

See [`docs/architecture.md`](../../docs/architecture.md) for the full
architecture.
