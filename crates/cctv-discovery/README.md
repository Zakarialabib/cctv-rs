# cctv-discovery

Camera discovery for the `cctv-rs` workspace: the SSDP `M-SEARCH` sweep, the
multicast sweep and the USB-bus probe, behind one read-only surface.

Discovery is unauthenticated and protocol-thin. A sweep finds cameras and hands each
one back as a `DiscoveredCamera` — the stable identity discovery observed plus a
`cctv-core` `ProbeResult` — so the fleet can adopt a device without knowing the
vendor's protocol. This crate never speaks TLS and never controls a camera.

Public surface:

- `Probe` — a stateless probe; each `Probe::scan` is one sweep.
- `DiscoveredCamera` — one camera found by a sweep: its stable identity and the
  device-model probe fields.

Until the transports are wired, `Probe::scan` returns an empty sweep rather than
pretending to have found a device.

The design, the protocol matrix and the crate's place in the workspace are in
[`docs/architecture.md`](../../docs/architecture.md).
