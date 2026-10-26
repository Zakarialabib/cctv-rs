# cctv-onvif

The ONVIF driver for `cctv-rs` — the one crate that speaks ONVIF. Device, Media
and PTZ services over SOAP 1.2, authenticated with a WS-Security
`UsernameToken`. The ONVIF vocabulary (profiles, tokens, `XAddr`) stops at this
crate's boundary: everything the rest of the workspace sees is a `cctv-core`
type.

## Layout

| Module | What it holds |
| --- | --- |
| `soap` | Envelopes, the `#PasswordDigest` header, ISO 8601 timestamps. |
| `device` | `GetDeviceInformation`, `GetCapabilities`. |
| `media` | Profiles, stream / snapshot URIs, the encoder write path. |
| `ptz` | Continuous / absolute moves, stop, presets. |
| `session` | `OnvifSession` — one device on the wire. |
| `backend` | `OnvifCameraBackend` — one camera as a `CameraBackend`. |
| `error` | `OnvifError` and its mapping onto `CameraError`. |

Builders return SOAP **body** fragments; `soap::envelope` wraps them. Parsers are
namespace-prefix-agnostic, so `tds:`, `SOAP-ENV:` and unprefixed replies are read
alike.

## The session

`OnvifSession` is where the builders meet the wire: each call wraps its body in
an envelope with a fresh WS-Security header and POSTs it through
`cctv-transport`'s shared client, which already negotiates HTTP Digest / Basic
for the devices that want that *in addition to* message-level auth.

```rust,no_run
use cctv_onvif::{Credentials, OnvifSession};
use cctv_transport::http::HttpConfig;

# async fn run() -> Result<(), cctv_onvif::OnvifError> {
let session = OnvifSession::new(
    "http://192.168.1.64/onvif/device_service",
    Credentials { username: "admin".into(), password: "secret".into() },
    HttpConfig::default(),
)?;
let info = session.device_information().await?;
let profiles = session.profiles().await?;
# Ok(())
# }
```

Service endpoints come from `GetCapabilities` and are **cached**: a device
answers `XAddr`s that differ from its device-service URL, so every later call
has to address the right one — but only the first has to ask.

## Two behaviours worth knowing

- **`Stop` names a profile.** ONVIF's `Stop` takes a profile token; a
  device-model `PtzCommand::Stop` does not. `OnvifCameraBackend` remembers the
  profile that started moving, and a stop with nothing moving is a no-op rather
  than an error.
- **A snapshot is two requests.** `GetSnapshotUri` returns a *URL*; the bytes
  come from a plain `GET` against the device's own HTTP server. `snapshot()`
  does both.

## Testing

`cargo test -p cctv-onvif` runs the builders / parsers, the WS-Security digest
and the conformance suite against a canned SOAP server (`cctv-transport`'s
fixture) — no camera, no credentials, no network. The conformance run is
**7/7** with `Expectations::FULL`: identity, both streams, a snapshot, a move +
stop, and the preset round trip.

## What this crate does not do

No ONVIF event subscription (`PullPoint` / `BasicNotificationInterface`), no
imaging / analytics services, and no device time (`GetSystemDateAndTime`) — a
reply's timestamps are not used for anything yet. Those are unimplemented, not
faked.
