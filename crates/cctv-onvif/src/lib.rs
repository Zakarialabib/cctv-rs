//! ONVIF driver for cctv-rs — protocol only.
//!
//! SOAP 1.2 request builders and reply parsers for the ONVIF **Device**,
//! **Media** and **PTZ** services, the WS-Security `UsernameToken` header, and
//! the session that drives them over `cctv-transport`'s shared HTTP client.  The
//! ONVIF vocabulary — profiles, tokens, `XAddr` — stops at this crate's boundary:
//! everything the rest of the workspace sees is a `cctv-core` type.
//!
//! - [`soap`] — envelopes, the `#PasswordDigest` header, ISO 8601 timestamps.
//! - [`device`] — `GetDeviceInformation` and `GetCapabilities`.
//! - [`media`] — profiles, stream / snapshot URIs, the encoder write path.
//! - [`ptz`] — moves, stop and presets.
//! - [`session`] — one device on the wire: the builders above, POSTed.
//! - [`backend`] — one camera as a [`cctv_core::CameraBackend`], which is what
//!   the facade and the conformance suite drive.
//! - [`error`] — [`OnvifError`] and its mapping onto [`cctv_core::CameraError`].
//!
//! Builders return SOAP **body** fragments; [`soap::envelope`] wraps them.
//! Parsers are namespace-prefix-agnostic, so `tds:`, `SOAP-ENV:` and unprefixed
//! replies are read alike.

#![forbid(unsafe_code)]

pub mod backend;
pub mod device;
pub mod error;
pub mod media;
pub mod ptz;
pub mod session;
pub mod soap;

pub use backend::OnvifCameraBackend;
pub use error::OnvifError;
pub use session::OnvifSession;
pub use soap::Credentials;
