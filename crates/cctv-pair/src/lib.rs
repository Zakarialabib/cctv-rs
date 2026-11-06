//! Device pairing and trust store for `cctv-rs`.
//!
//! Provides the pairing ceremony state machine, QR URI parsing/formatting,
//! numeric comparison with cryptographic commitments, ADB host transport integration,
//! and secure trust storage with rate-limiting / lockout defenses.

#![forbid(unsafe_code)]

pub mod adb;
pub mod ceremony;
pub mod crypto;
pub mod trust;
pub mod uri;
pub mod zbase32;

pub use adb::{
    AdbError, AdbHostSession, AdbMessage, AdbMessageHeader, AdbSessionState, A_CLSE, A_CNXN,
    A_OKAY, A_OPEN, A_SYNC, A_VERSION, A_WRTE, MAX_ADB_PAYLOAD,
};

pub use ceremony::{
    from_hex, to_hex, CeremonyConfig, CeremonyRole, CeremonyState, PairingCeremony, PairingError,
    PairingMessage, PairingMethod, CURRENT_PROTOCOL_VERSION, PAIRING_ALPN,
};

pub use crypto::{
    compute_commitment, compute_pairing_proof, compute_sas, constant_time_eq, hmac_sha256, sha256,
    verify_commitment, verify_pairing_proof,
};
pub use trust::{
    LockoutManager, TrustStore, TrustedPeer, CEREMONY_TIMEOUT, LOCKOUT_DURATION,
    MAX_FAILED_ATTEMPTS,
};
pub use uri::{PairingQrPayload, QrUriError};
