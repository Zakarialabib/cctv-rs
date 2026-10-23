# Security policy

## Reporting a vulnerability

Do not open a public issue. Report privately through GitHub's
[Security Advisories](https://github.com/Zakarialabib/cctv-rs/security/advisories/new)
so a fix can be prepared before disclosure.

Please include the affected crate and version, the protocol involved (Ezviz Open
Platform, RTSP, MJPEG, ONVIF PTZ, SSDP discovery, TLS transport) and a
reproduction. Expect an acknowledgement within 72 hours.

## Scope

This workspace talks to cameras over the local network and to the Ezviz cloud,
and it handles camera credentials. Reports in these areas are especially
relevant:

- **Credential handling** — Ezviz app keys, access tokens and RTSP
  user/password pairs reach the driver through `cctv-ezviz`. Accidental logging
  of a token, writing one to a world-readable path, or embedding one in an error
  message is in scope.
- **TLS policy** — `cctv-transport` owns certificate pinning and the minimum TLS
  version. A pin that can be bypassed, a fallback to an unverified connection, or
  a policy that silently degrades to `TlsVersion::V1_2` when it was configured
  pinned is in scope.
- **Stream handling** — `cctv-transport` reads frames under a budget. A malicious
  or compromised camera that can exhaust memory, stall the fleet or desynchronise
  the reader is in scope. Snapshot paths written by the caller must reject
  traversal (`../`).
- **Denial of service** — unbounded SSDP/discovery response parsing in
  `cctv-discovery`, or a frame budget a LAN peer can exhaust.
- **Unsafe code** — there is no `unsafe` in this workspace; every crate root
  forbids it. An `unsafe` block, or unsoundness in FFI reached through a
  dependency, is in scope.

## Out of scope

- Anything requiring physical access to a camera or its firmware.
- Capabilities that are deliberately unimplemented — they fail closed with a
  typed `CameraError`, which is the intended behaviour.
- Vulnerabilities in the vendor SDK (`OpenNetStream.dll`) or in
  `ezviz-openapi-rs`; report those to their maintainers. This workspace never
  redistributes vendor binaries.
