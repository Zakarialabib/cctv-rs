# cctv-onvif

The ONVIF driver for `cctv-rs` — **protocol only**. It turns the ONVIF Device,
Media and PTZ services into SOAP 1.2 requests and reads the replies back into
`cctv-core` types; the ONVIF vocabulary (profiles, tokens, `XAddr`) never leaves
this crate.

Auth is WS-Security at the message level: a `UsernameToken` whose password is
`Base64(SHA1(nonce ‖ created ‖ password))`, layered on the Digest / Basic
negotiation `cctv-transport` already speaks.

- `soap` — the SOAP 1.2 envelope, the `#PasswordDigest` header and ISO 8601
  timestamps. `Credentials::header`, `password_digest`, `security_header`,
  `envelope`, `iso8601_utc`.
- `device` — `GetDeviceInformation` / `GetCapabilities`:
  `parse_device_information`, `parse_capabilities` (the Media and PTZ `XAddr`).
- `media` — `GetProfiles`, `GetStreamUri`, `GetSnapshotUri` and
  `SetVideoEncoderConfiguration`; `parse_profiles`, `parse_media_uri`,
  `pick_main_profile` / `pick_substream_profile`, `encoder_config_token`.
- `ptz` — `ContinuousMove`, `AbsoluteMove`, `Stop`, `GetPresets`, `SetPreset`,
  `RemovePreset`, `GotoPreset`; `parse_presets`.
- `error` — `OnvifError` and its mapping onto `cctv_core::CameraError`.

Builders return SOAP **body** fragments; `soap::envelope` wraps them. Parsers are
namespace-prefix-agnostic, so `tds:`, `SOAP-ENV:` and unprefixed replies are read
alike. Stream URIs are *enumerated* through `GetProfiles`, never guessed —
`/stream1`, `/axis-media/media.amp` and `/cam/realmonitor?channel=1&subtype=0`
are all one vendor's convention.

See [`docs/architecture.md`](../../docs/architecture.md) for the full
architecture.
