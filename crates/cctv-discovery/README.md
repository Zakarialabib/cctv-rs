# cctv-discovery

Camera discovery for the `cctv-rs` workspace: the SSDP `M-SEARCH` sweep and the
WS-Discovery `Probe` sweep, over UDP multicast, behind one read-only surface.

Discovery is unauthenticated and protocol-thin. A sweep finds cameras and hands
each one back as a `DiscoveredCamera` — the identity discovery observed, the
`cctv-core` `ProbeResult` fields, and the control-service URL when the protocol
advertised one — so the fleet can adopt a device without knowing the vendor's
protocol. This crate never speaks TLS and never controls a camera.

## You get

- `Probe` — a stateless probe. `Probe::scan` is one sweep over a real UDP socket;
  `Probe::scan_with` is the same sweep over an injected `Datagram`.
- `DiscoveredCamera` — one camera found by a sweep: its identity, its probe
  fields, and the advertised control-service URL.
- `Datagram` / `UdpTransport` — the transport seam a sweep runs on.
- `ssdp` — the `M-SEARCH` request and the reply codec (pure).
- `wsd` — the WS-Discovery `Probe` envelope and the `ProbeMatches` parser (pure;
  reads replies with `cctv-transport`'s prefix-agnostic XML scanner).

Both protocols are tried and the results merged, deduped by identity and ordered
by it. An unreachable network yields an empty sweep — never an error, never a
fabricated device.

USB cameras are enumerated by the **application**: a USB camera is a kernel /
hardware concern, not a protocol, so this crate does not touch the USB bus.

The design, the protocol matrix and the crate's place in the workspace are in
`docs/architecture.md`.
