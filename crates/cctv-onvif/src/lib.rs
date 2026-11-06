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
//! - [`discovery`] — WS-Discovery sweeps delegated to `cctv-discovery`,
//!   filtered to devices with ONVIF evidence and an addressable `XAddr`.
//! - [`backend`] — one camera as a [`cctv_core::CameraBackend`], which is what
//!   the facade and the conformance suite drive.
//! - [`error`] — [`OnvifError`] and its mapping onto [`cctv_core::CameraError`].
//!
//! Builders return SOAP **body** fragments; [`soap::envelope`] wraps them.
//! Parsers are namespace-prefix-agnostic, so `tds:`, `SOAP-ENV:` and unprefixed
//! replies are read alike.

#![forbid(unsafe_code)]

pub mod auth;
pub mod backend;
pub mod device;
pub mod discovery;
pub mod error;
pub mod events;
pub mod media;
pub mod ptz;
pub mod session;
pub mod soap;

pub use backend::OnvifCameraBackend;
pub use discovery::{Discoverable, OnvifDiscovery};
pub use error::OnvifError;
pub use events::{
    build_create_pull_point_subscription, build_pull_messages, build_renew, build_unsubscribe,
    format_iso_duration, parse_create_pull_point_subscription, parse_iso_duration,
    parse_pull_messages, parse_renew, subscribe_events, EventSubscription, OnvifEvent,
    PullPointConfig, RenewResponse, SubscriptionReference, EVENTS_NS, TOPIC_NS, WSA_NS, WSE_NS,
    WSNT_NS,
};
pub use ptz::{
    build_absolute_move, build_continuous_move, build_get_presets, build_goto_preset,
    build_remove_preset, build_set_preset, build_set_system_date_and_time_soap, build_stop,
    parse_presets, parse_set_preset, set_system_date_and_time,
};
pub use session::OnvifSession;
pub use soap::Credentials;
