//! ONVIF driver for cctv-rs — protocol only.
//!
//! SOAP 1.2 request builders and reply parsers for the ONVIF **Device**,
//! **Media** and **PTZ** services, plus the WS-Security `UsernameToken` header
//! ([`soap::Credentials`]).  The ONVIF vocabulary — profiles, tokens, `XAddr` —
//! stops at this crate's boundary: everything the rest of the workspace sees is
//! a `cctv-core` type.
//!
//! - [`soap`] — envelopes, the `#PasswordDigest` header, ISO 8601 timestamps.
//! - [`device`] — `GetDeviceInformation` and `GetCapabilities`.
//! - [`media`] — profiles, stream / snapshot URIs, the encoder write path.
//! - [`ptz`] — moves, stop and presets.
//! - [`error`] — [`OnvifError`] and its mapping onto [`cctv_core::CameraError`].
//!
//! Builders return SOAP **body** fragments; [`soap::envelope`] wraps them.
//! Parsers are namespace-prefix-agnostic, so `tds:`, `SOAP-ENV:` and unprefixed
//! replies are read alike.

#![forbid(unsafe_code)]

pub mod device;
pub mod error;
pub mod media;
pub mod ptz;
pub mod soap;

pub use error::OnvifError;
