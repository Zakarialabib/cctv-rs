//! PairingCeremony state machine for device pairing and trust establishment.
//!
//! Protocols supported:
//! 1. QR flow:
//!    - Initiator/Client scans QR URI: `cctv://pair?v=0&id=<zbase32>&s=<b64url>&n=<pct-encoded>&a=<ip:port>,...`
//!    - Client dials with ALPN `cctv-pair/0`
//!    - Sends `{device, proof: HMAC-SHA256(S, "cctv-pair-v0" ‖ client_id ‖ host_id)}`
//!    - Host verifies proof
//!    - Host responds with its device identity / host proof
//!    - Both parties exchange `pair.done` ack
//!    - Close with code 0. Initiator stores trust only on close code 0.
//!
//! 2. Numeric-compare flow (SAS):
//!    - Commitment: A generates `nA` and commits `c = SHA-256(nA)`. Sends `Commitment { c, idA }`.
//!    - Nonce exchange: B generates `nB` and sends `NonceB { nB, idB }`.
//!    - Nonce reveal: A reveals `nA`. B verifies `c == SHA-256(nA)`.
//!    - Both compute `sas = uint32(HMAC-SHA256(nA ‖ nB, "cctv-sas-v0" ‖ idA ‖ idB)[0..4]) mod 1_000_000`.
//!    - Both display/confirm match.
//!    - Send `pair.done` ack.
//!    - Close with code 0. Initiator stores trust only on close code 0.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::crypto::{
    compute_commitment, compute_pairing_proof, compute_sas, verify_commitment, verify_pairing_proof,
};
use crate::trust::{TrustStore, TrustedPeer, CEREMONY_TIMEOUT};

/// ALPN identifier for pairing protocol.
pub const PAIRING_ALPN: &[u8] = b"cctv-pair/0";

/// Ceremony role: Initiator (client dialing or connecting) or Responder (host listening).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CeremonyRole {
    /// Device initiating connection and scanning QR or starting SAS.
    Initiator,
    /// Device hosting QR or responding to SAS ceremony.
    Responder,
}

/// Pairing method used during ceremony.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingMethod {
    /// QR code with pre-shared secret.
    QrCode,
    /// Numeric comparison with commitment.
    NumericCompare,
    /// USB via ADB protocol.
    UsbAdb,
}

/// Ceremony state machine states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CeremonyState {
    /// Idle before starting.
    Idle,
    /// QR Flow: Client dialed, sent DeviceProof, awaiting HostAck.
    QrAwaitingHostResponse,
    /// QR Flow: Host received Client proof, validated, sent response, awaiting DoneAck.
    QrAwaitingDoneAck,
    /// Numeric Compare: Awaiting peer's commitment or nonce.
    SasAwaitingCommitment,
    /// Numeric Compare: Received commitment, sent own nonce, awaiting reveal.
    SasAwaitingReveal,
    /// Numeric Compare: Both nonces verified, SAS calculated, awaiting user confirmation.
    SasAwaitingUserConfirmation {
        /// The calculated 6-digit SAS value.
        sas: u32,
    },
    /// Numeric Compare: User confirmed, awaiting remote pair.done ack.
    SasAwaitingDoneAck,
    /// Ceremony completed successfully, awaiting close frame with code 0.
    AwaitingClose,
    /// Successfully paired and closed with code 0.
    Success(TrustedPeer),
    /// Failed with error reason.
    Failed(PairingError),
}

/// Protocol version constants to prevent downgrade attacks.
pub const CURRENT_PROTOCOL_VERSION: u32 = 1;

/// Pairing wire messages exchanged over the pairing transport.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "payload")]
pub enum PairingMessage {
    /// QR flow: Client connects and sends its identity and HMAC proof with monotonic counter.
    QrClientHello {
        /// Protocol version (enforced against downgrade attacks).
        #[serde(default = "default_protocol_version")]
        version: u32,
        /// Client device identifier.
        client_id: String,
        /// Client friendly name.
        client_name: String,
        /// Strictly monotonically increasing nonce counter.
        #[serde(default)]
        nonce_counter: u64,
        /// Hex-encoded HMAC-SHA256 proof.
        proof_hex: String,
    },
    /// QR flow: Host verifies client proof and replies with host identity.
    QrHostResponse {
        /// Protocol version (enforced against downgrade attacks).
        #[serde(default = "default_protocol_version")]
        version: u32,
        /// Host device identifier.
        host_id: String,
        /// Host friendly name.
        host_name: String,
        /// Monotonically increasing nonce counter matching or advancing the challenge.
        #[serde(default)]
        nonce_counter: u64,
        /// Optional reciprocal proof: HMAC-SHA256(S, "cctv-pair-v0" ‖ host_id ‖ client_id ‖ nonce_counter).
        proof_hex: String,
    },
    /// SAS flow: Initiator commits `c = SHA256(nA)`.
    SasCommitment {
        /// Sender device identifier.
        sender_id: String,
        /// Sender friendly name.
        sender_name: String,
        /// Hex-encoded 32-byte commitment.
        commitment_hex: String,
    },
    /// SAS flow: Responder sends `nB`.
    SasNonceB {
        /// Sender device identifier.
        sender_id: String,
        /// Sender friendly name.
        sender_name: String,
        /// Hex-encoded nonce B (typically 16-32 bytes).
        nonce_hex: String,
    },
    /// SAS flow: Initiator reveals `nA`.
    SasRevealA {
        /// Hex-encoded nonce A matching commitment.
        nonce_hex: String,
    },
    /// Both flows: Acknowledgement that user confirmed pairing.
    PairDone {
        /// Reason or confirmation string (e.g. "pair.done").
        status: String,
    },
    /// Close code message closing the ceremony session.
    Close {
        /// Close status code: 0 indicates clean success; non-zero indicates error.
        code: u32,
        /// Error or status description.
        reason: String,
    },
}

/// Errors occurring during pairing ceremonies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingError {
    /// Ceremony timed out (exceeded 5 minutes).
    Timeout,
    /// Forged or invalid HMAC proof received.
    ForgedProof,
    /// Nonce counter was not strictly monotonically increasing (replay attack detected).
    ReplayDetected(u64, u64),
    /// Protocol downgrade attack detected or unsupported version.
    DowngradeDetected(u32),
    /// Commitment mismatch (nonce did not hash to committed value).
    CommitmentMismatch,
    /// Numeric compare (SAS) code was rejected or mismatched by user.
    SasMismatch,
    /// Peer closed connection with non-zero code.
    NonZeroCloseCode(u32, String),
    /// Protocol state error (unexpected message).
    UnexpectedMessage(String),
    /// Invalid wire encoding / hex format.
    InvalidEncoding(String),
    /// Session was aborted.
    Aborted(String),
}

impl fmt::Display for PairingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => write!(f, "pairing ceremony timed out after 5 minutes"),
            Self::ForgedProof => write!(f, "forged or invalid pairing proof"),
            Self::ReplayDetected(received, expected_min) => {
                write!(
                    f,
                    "replay attack detected: nonce counter {received} <= expected {expected_min}"
                )
            }
            Self::DowngradeDetected(v) => {
                write!(f, "protocol downgrade attack detected: version {v}")
            }
            Self::CommitmentMismatch => write!(f, "cryptographic commitment mismatch"),
            Self::SasMismatch => write!(f, "numeric compare SAS code rejected by user"),
            Self::NonZeroCloseCode(code, msg) => {
                write!(f, "closed with error code {code}: {msg}")
            }
            Self::UnexpectedMessage(msg) => write!(f, "unexpected pairing message: {msg}"),
            Self::InvalidEncoding(msg) => write!(f, "invalid message encoding: {msg}"),
            Self::Aborted(msg) => write!(f, "pairing ceremony aborted: {msg}"),
        }
    }
}

impl std::error::Error for PairingError {}

fn default_protocol_version() -> u32 {
    CURRENT_PROTOCOL_VERSION
}

/// Configuration for running a pairing ceremony.
#[derive(Clone)]
pub struct CeremonyConfig {
    /// Protocol version.
    pub version: u32,
    /// Local device identifier.
    pub local_id: String,
    /// Local friendly name.
    pub local_name: String,
    /// Ceremony role.
    pub role: CeremonyRole,
    /// Pairing method.
    pub method: PairingMethod,
    /// Pre-shared secret (for QR flow).
    pub secret: Option<Zeroizing<Vec<u8>>>,
    /// Remote peer identifier (if known in advance, e.g. from QR URI).
    pub expected_remote_id: Option<String>,
    /// Outbound nonce counter to attach.
    pub initial_nonce_counter: u64,
    /// Minimum expected incoming nonce counter (must be strictly monotonically increasing).
    pub min_expected_nonce: u64,
    /// Maximum ceremony duration (default 5 minutes).
    pub timeout: Duration,
}

impl CeremonyConfig {
    /// Create a QR ceremony config for initiator (client scanning QR).
    pub fn qr_initiator(
        client_id: impl Into<String>,
        client_name: impl Into<String>,
        host_id: impl Into<String>,
        secret: Vec<u8>,
    ) -> Self {
        Self {
            version: CURRENT_PROTOCOL_VERSION,
            local_id: client_id.into(),
            local_name: client_name.into(),
            role: CeremonyRole::Initiator,
            method: PairingMethod::QrCode,
            secret: Some(Zeroizing::new(secret)),
            expected_remote_id: Some(host_id.into()),
            initial_nonce_counter: 1,
            min_expected_nonce: 0,
            timeout: CEREMONY_TIMEOUT,
        }
    }

    /// Create a QR ceremony config for initiator with a custom initial nonce counter.
    pub fn qr_initiator_with_nonce(
        client_id: impl Into<String>,
        client_name: impl Into<String>,
        host_id: impl Into<String>,
        secret: Vec<u8>,
        initial_nonce: u64,
    ) -> Self {
        let mut cfg = Self::qr_initiator(client_id, client_name, host_id, secret);
        cfg.initial_nonce_counter = initial_nonce;
        cfg
    }

    /// Create a QR ceremony config for responder (host displaying QR).
    pub fn qr_responder(
        host_id: impl Into<String>,
        host_name: impl Into<String>,
        secret: Vec<u8>,
    ) -> Self {
        Self {
            version: CURRENT_PROTOCOL_VERSION,
            local_id: host_id.into(),
            local_name: host_name.into(),
            role: CeremonyRole::Responder,
            method: PairingMethod::QrCode,
            secret: Some(Zeroizing::new(secret)),
            expected_remote_id: None,
            initial_nonce_counter: 1,
            min_expected_nonce: 0,
            timeout: CEREMONY_TIMEOUT,
        }
    }

    /// Create a QR ceremony config for responder with a persisted minimum nonce threshold.
    pub fn qr_responder_with_persisted_nonce(
        host_id: impl Into<String>,
        host_name: impl Into<String>,
        secret: Vec<u8>,
        min_expected_nonce: u64,
    ) -> Self {
        let mut cfg = Self::qr_responder(host_id, host_name, secret);
        cfg.min_expected_nonce = min_expected_nonce;
        cfg
    }

    /// Create a Numeric Compare ceremony config.
    pub fn numeric_compare(
        local_id: impl Into<String>,
        local_name: impl Into<String>,
        role: CeremonyRole,
    ) -> Self {
        Self {
            version: CURRENT_PROTOCOL_VERSION,
            local_id: local_id.into(),
            local_name: local_name.into(),
            role,
            method: PairingMethod::NumericCompare,
            secret: None,
            expected_remote_id: None,
            initial_nonce_counter: 1,
            min_expected_nonce: 0,
            timeout: CEREMONY_TIMEOUT,
        }
    }
}

/// The pairing ceremony state machine.
pub struct PairingCeremony {
    config: CeremonyConfig,
    state: CeremonyState,
    started_at_ms: u64,

    // Monotonic nonce counter state
    local_nonce_counter: u64,
    last_verified_remote_nonce: u64,

    // Internal context
    remote_id: Option<String>,
    remote_name: Option<String>,
    local_nonce: Option<Zeroizing<Vec<u8>>>,
    remote_commitment: Option<[u8; 32]>,
    remote_nonce: Option<Vec<u8>>,
    computed_sas: Option<u32>,
}

impl PairingCeremony {
    /// Start a new pairing ceremony.
    pub fn new(config: CeremonyConfig, start_time_ms: u64) -> Self {
        let state = CeremonyState::Idle;
        let local_nonce_counter = config.initial_nonce_counter;
        let last_verified_remote_nonce = config.min_expected_nonce;
        Self {
            config,
            state,
            started_at_ms: start_time_ms,
            local_nonce_counter,
            last_verified_remote_nonce,
            remote_id: None,
            remote_name: None,
            local_nonce: None,
            remote_commitment: None,
            remote_nonce: None,
            computed_sas: None,
        }
    }

    /// Current ceremony state.
    pub fn state(&self) -> &CeremonyState {
        &self.state
    }

    /// Current local nonce counter value.
    pub fn local_nonce_counter(&self) -> u64 {
        self.local_nonce_counter
    }

    /// Last verified remote nonce counter.
    pub fn last_verified_remote_nonce(&self) -> u64 {
        self.last_verified_remote_nonce
    }

    /// Check if ceremony has expired based on current timestamp.
    pub fn check_timeout(&mut self, now_ms: u64) -> bool {
        if matches!(
            self.state,
            CeremonyState::Success(_) | CeremonyState::Failed(_)
        ) {
            return false;
        }

        let elapsed_ms = now_ms.saturating_sub(self.started_at_ms);
        if elapsed_ms > self.config.timeout.as_millis() as u64 {
            self.state = CeremonyState::Failed(PairingError::Timeout);
            true
        } else {
            false
        }
    }

    /// Generate initial outbound message for initiator, if applicable.
    pub fn start_initiation(
        &mut self,
        nonce_bytes: Option<Vec<u8>>,
    ) -> Result<Option<PairingMessage>, PairingError> {
        if self.config.role != CeremonyRole::Initiator {
            return Ok(None);
        }

        match self.config.method {
            PairingMethod::QrCode | PairingMethod::UsbAdb => {
                let secret = self
                    .config
                    .secret
                    .as_ref()
                    .ok_or_else(|| PairingError::Aborted("missing secret".into()))?;
                let host_id = self
                    .config
                    .expected_remote_id
                    .as_deref()
                    .ok_or_else(|| PairingError::Aborted("missing expected host id".into()))?;

                let nonce = self.local_nonce_counter;
                let proof = compute_pairing_proof(secret, &self.config.local_id, host_id, nonce);
                self.state = CeremonyState::QrAwaitingHostResponse;
                self.remote_id = Some(host_id.to_string());

                Ok(Some(PairingMessage::QrClientHello {
                    version: self.config.version,
                    client_id: self.config.local_id.clone(),
                    client_name: self.config.local_name.clone(),
                    nonce_counter: nonce,
                    proof_hex: to_hex(&proof),
                }))
            }
            PairingMethod::NumericCompare => {
                let nonce = nonce_bytes.unwrap_or_else(|| vec![42u8; 16]);
                let commitment = compute_commitment(&nonce);
                self.local_nonce = Some(Zeroizing::new(nonce));
                self.state = CeremonyState::SasAwaitingCommitment;

                Ok(Some(PairingMessage::SasCommitment {
                    sender_id: self.config.local_id.clone(),
                    sender_name: self.config.local_name.clone(),
                    commitment_hex: to_hex(&commitment),
                }))
            }
        }
    }

    /// Process an incoming message and return an optional outbound response.
    pub fn handle_message(
        &mut self,
        msg: PairingMessage,
        now_ms: u64,
        responder_nonce: Option<Vec<u8>>,
    ) -> Result<Option<PairingMessage>, PairingError> {
        if self.check_timeout(now_ms) {
            return Err(PairingError::Timeout);
        }

        if let CeremonyState::Failed(ref err) = self.state {
            return Err(err.clone());
        }

        let msg_debug = format!("{msg:?}");
        match (&self.state, msg) {
            // --- QR Flow: Responder receives QrClientHello ---
            (
                CeremonyState::Idle,
                PairingMessage::QrClientHello {
                    version,
                    client_id,
                    client_name,
                    nonce_counter,
                    proof_hex,
                },
            ) => {
                if self.config.role != CeremonyRole::Responder {
                    return self.fail(PairingError::UnexpectedMessage("not responder".into()));
                }

                // Enforce version check against downgrade attack
                if version < self.config.version {
                    return self.fail(PairingError::DowngradeDetected(version));
                }

                // Enforce strictly monotonically increasing nonce counter against replay attack
                if nonce_counter <= self.last_verified_remote_nonce {
                    return self.fail(PairingError::ReplayDetected(
                        nonce_counter,
                        self.last_verified_remote_nonce,
                    ));
                }

                let secret = self
                    .config
                    .secret
                    .as_ref()
                    .ok_or_else(|| PairingError::Aborted("missing secret".into()))?;
                let proof_bytes = from_hex(&proof_hex)
                    .map_err(|e| PairingError::InvalidEncoding(e.to_string()))?;
                if proof_bytes.len() != 32 {
                    return self.fail(PairingError::ForgedProof);
                }
                let mut proof_arr = [0u8; 32];
                proof_arr.copy_from_slice(&proof_bytes);

                // Verify: HMAC-SHA256(S, "cctv-pair-v0" ‖ client_id ‖ host_id ‖ nonce_counter)
                let valid = verify_pairing_proof(
                    secret,
                    &client_id,
                    &self.config.local_id,
                    nonce_counter,
                    &proof_arr,
                );
                if !valid {
                    return self.fail(PairingError::ForgedProof);
                }

                self.remote_id = Some(client_id.clone());
                self.remote_name = Some(client_name);
                self.last_verified_remote_nonce = nonce_counter;

                // Reciprocal host proof increments counter or echoes challenge
                let host_nonce = nonce_counter;
                let host_proof =
                    compute_pairing_proof(secret, &self.config.local_id, &client_id, host_nonce);
                self.state = CeremonyState::QrAwaitingDoneAck;

                Ok(Some(PairingMessage::QrHostResponse {
                    version: self.config.version,
                    host_id: self.config.local_id.clone(),
                    host_name: self.config.local_name.clone(),
                    nonce_counter: host_nonce,
                    proof_hex: to_hex(&host_proof),
                }))
            }

            // --- QR Flow: Initiator receives QrHostResponse ---
            (
                CeremonyState::QrAwaitingHostResponse,
                PairingMessage::QrHostResponse {
                    version,
                    host_id,
                    host_name,
                    nonce_counter,
                    proof_hex,
                },
            ) => {
                // Enforce version check against downgrade attack
                if version < self.config.version {
                    return self.fail(PairingError::DowngradeDetected(version));
                }

                if let Some(ref exp_host) = self.config.expected_remote_id {
                    if exp_host != &host_id {
                        return self.fail(PairingError::ForgedProof);
                    }
                }

                // Verify responder's nonce matches the initiator's challenge counter
                if nonce_counter != self.local_nonce_counter {
                    return self.fail(PairingError::ReplayDetected(
                        nonce_counter,
                        self.local_nonce_counter,
                    ));
                }

                let secret = self
                    .config
                    .secret
                    .as_ref()
                    .ok_or_else(|| PairingError::Aborted("missing secret".into()))?;
                let proof_bytes = from_hex(&proof_hex)
                    .map_err(|e| PairingError::InvalidEncoding(e.to_string()))?;
                if proof_bytes.len() != 32 {
                    return self.fail(PairingError::ForgedProof);
                }
                let mut proof_arr = [0u8; 32];
                proof_arr.copy_from_slice(&proof_bytes);

                // Verify reciprocal host proof
                let valid = verify_pairing_proof(
                    secret,
                    &host_id,
                    &self.config.local_id,
                    nonce_counter,
                    &proof_arr,
                );
                if !valid {
                    return self.fail(PairingError::ForgedProof);
                }

                self.remote_id = Some(host_id);
                self.remote_name = Some(host_name);
                self.last_verified_remote_nonce = nonce_counter;
                self.state = CeremonyState::AwaitingClose;

                // Send pair.done ack
                Ok(Some(PairingMessage::PairDone {
                    status: "pair.done".to_string(),
                }))
            }

            // --- QR Flow: Responder receives PairDone ---
            (CeremonyState::QrAwaitingDoneAck, PairingMessage::PairDone { .. }) => {
                let peer = TrustedPeer {
                    device_id: self.remote_id.clone().unwrap_or_default(),
                    name: self.remote_name.clone().unwrap_or_default(),
                    psk: self.config.secret.clone(),
                    paired_at_ms: now_ms,
                };
                self.state = CeremonyState::Success(peer);

                // Close code 0 to client
                Ok(Some(PairingMessage::Close {
                    code: 0,
                    reason: "pairing successful".to_string(),
                }))
            }

            // --- SAS Flow: Responder receives SasCommitment ---
            (
                CeremonyState::Idle,
                PairingMessage::SasCommitment {
                    sender_id,
                    sender_name,
                    commitment_hex,
                },
            ) => {
                if self.config.role != CeremonyRole::Responder {
                    return self.fail(PairingError::UnexpectedMessage("not responder".into()));
                }

                let commit_bytes = from_hex(&commitment_hex)
                    .map_err(|e| PairingError::InvalidEncoding(e.to_string()))?;
                if commit_bytes.len() != 32 {
                    return self.fail(PairingError::InvalidEncoding(
                        "invalid commitment len".into(),
                    ));
                }
                let mut commit_arr = [0u8; 32];
                commit_arr.copy_from_slice(&commit_bytes);

                self.remote_id = Some(sender_id);
                self.remote_name = Some(sender_name);
                self.remote_commitment = Some(commit_arr);

                let nonce_b = responder_nonce.unwrap_or_else(|| vec![77u8; 16]);
                self.local_nonce = Some(Zeroizing::new(nonce_b.clone()));
                self.state = CeremonyState::SasAwaitingReveal;

                Ok(Some(PairingMessage::SasNonceB {
                    sender_id: self.config.local_id.clone(),
                    sender_name: self.config.local_name.clone(),
                    nonce_hex: to_hex(&nonce_b),
                }))
            }

            // --- SAS Flow: Initiator receives SasNonceB ---
            (
                CeremonyState::SasAwaitingCommitment,
                PairingMessage::SasNonceB {
                    sender_id,
                    sender_name,
                    nonce_hex,
                },
            ) => {
                let nb = from_hex(&nonce_hex)
                    .map_err(|e| PairingError::InvalidEncoding(e.to_string()))?;
                self.remote_id = Some(sender_id);
                self.remote_name = Some(sender_name);
                self.remote_nonce = Some(nb.clone());

                let na = self.local_nonce.as_ref().unwrap();
                let sas = compute_sas(
                    na,
                    &nb,
                    &self.config.local_id,
                    self.remote_id.as_ref().unwrap(),
                );
                self.computed_sas = Some(sas);
                self.state = CeremonyState::SasAwaitingUserConfirmation { sas };

                // Reveal nA
                Ok(Some(PairingMessage::SasRevealA {
                    nonce_hex: to_hex(na),
                }))
            }

            // --- SAS Flow: Responder receives SasRevealA ---
            (CeremonyState::SasAwaitingReveal, PairingMessage::SasRevealA { nonce_hex }) => {
                let na = from_hex(&nonce_hex)
                    .map_err(|e| PairingError::InvalidEncoding(e.to_string()))?;
                let commit = self.remote_commitment.as_ref().unwrap();

                // Verify commitment: commitment == SHA256(nA)
                if !verify_commitment(commit, &na) {
                    return self.fail(PairingError::CommitmentMismatch);
                }

                let nb = self.local_nonce.as_ref().unwrap();
                let sas = compute_sas(
                    &na,
                    nb,
                    self.remote_id.as_ref().unwrap(),
                    &self.config.local_id,
                );
                self.remote_nonce = Some(na);
                self.computed_sas = Some(sas);
                self.state = CeremonyState::SasAwaitingUserConfirmation { sas };

                Ok(None)
            }

            // --- SAS Flow: Remote confirms PairDone ---
            (CeremonyState::SasAwaitingDoneAck, PairingMessage::PairDone { .. }) => {
                if self.config.role == CeremonyRole::Responder {
                    let peer = TrustedPeer {
                        device_id: self.remote_id.clone().unwrap_or_default(),
                        name: self.remote_name.clone().unwrap_or_default(),
                        psk: None,
                        paired_at_ms: now_ms,
                    };
                    self.state = CeremonyState::Success(peer);

                    Ok(Some(PairingMessage::Close {
                        code: 0,
                        reason: "sas pairing complete".to_string(),
                    }))
                } else {
                    self.state = CeremonyState::AwaitingClose;
                    Ok(None)
                }
            }

            // --- Close Message handling ---
            (_, PairingMessage::Close { code, reason }) => {
                if code == 0 {
                    let peer = TrustedPeer {
                        device_id: self.remote_id.clone().unwrap_or_default(),
                        name: self.remote_name.clone().unwrap_or_default(),
                        psk: self.config.secret.clone(),
                        paired_at_ms: now_ms,
                    };
                    self.state = CeremonyState::Success(peer);
                    Ok(None)
                } else {
                    self.fail(PairingError::NonZeroCloseCode(code, reason))
                }
            }

            _ => self.fail(PairingError::UnexpectedMessage(msg_debug)),
        }
    }

    /// User confirms numeric compare SAS code matches on screen.
    pub fn confirm_sas(
        &mut self,
        matched: bool,
        _now_ms: u64,
    ) -> Result<PairingMessage, PairingError> {
        if let CeremonyState::SasAwaitingUserConfirmation { .. } = self.state {
            if !matched {
                self.state = CeremonyState::Failed(PairingError::SasMismatch);
                return Ok(PairingMessage::Close {
                    code: 1,
                    reason: "SAS mismatch".to_string(),
                });
            }

            if self.config.role == CeremonyRole::Initiator {
                self.state = CeremonyState::AwaitingClose;
                Ok(PairingMessage::PairDone {
                    status: "pair.done".to_string(),
                })
            } else {
                self.state = CeremonyState::SasAwaitingDoneAck;
                Ok(PairingMessage::PairDone {
                    status: "pair.done".to_string(),
                })
            }
        } else {
            Err(PairingError::UnexpectedMessage(
                "not awaiting sas confirmation".into(),
            ))
        }
    }

    /// Finalize and store trust in TrustStore IF AND ONLY IF close code was 0 and state is Success.
    /// Also persists the updated monotonic nonce counter in TrustStore to prevent replay.
    pub fn commit_trust(&self, store: &mut TrustStore) -> Result<TrustedPeer, PairingError> {
        match &self.state {
            CeremonyState::Success(peer) => {
                store.insert(peer.clone());
                if self.last_verified_remote_nonce > 0 {
                    store.set_nonce_counter(&peer.device_id, self.last_verified_remote_nonce);
                }
                Ok(peer.clone())
            }
            CeremonyState::Failed(e) => Err(e.clone()),
            _ => Err(PairingError::Aborted("ceremony not completed".into())),
        }
    }

    fn fail<T>(&mut self, err: PairingError) -> Result<T, PairingError> {
        self.state = CeremonyState::Failed(err.clone());
        Err(err)
    }
}

/// Helper to format byte slice as lower-case hexadecimal string.
pub fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Helper to parse hexadecimal string into byte vector.
#[allow(clippy::manual_is_multiple_of)]
pub fn from_hex(hex: &str) -> Result<Vec<u8>, &'static str> {
    if hex.len() % 2 != 0 {
        return Err("odd hex string length");
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let bytes_raw = hex.as_bytes();
    for i in (0..bytes_raw.len()).step_by(2) {
        let hex_chunk = std::str::from_utf8(&bytes_raw[i..i + 2]).map_err(|_| "invalid utf8")?;
        let byte = u8::from_str_radix(hex_chunk, 16).map_err(|_| "invalid hex char")?;
        bytes.push(byte);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qr_flow_happy_path() {
        let secret = b"my-shared-qr-secret-key-42".to_vec();
        let client_id = "client-zbase32-id";
        let host_id = "host-zbase32-id";

        let client_cfg =
            CeremonyConfig::qr_initiator(client_id, "Mobile App", host_id, secret.clone());
        let host_cfg = CeremonyConfig::qr_responder(host_id, "Edge Hub", secret.clone());

        let mut client = PairingCeremony::new(client_cfg, 1000);
        let mut host = PairingCeremony::new(host_cfg, 1000);

        // Client initiates
        let client_hello = client
            .start_initiation(None)
            .unwrap()
            .expect("client hello");

        // Host handles client hello
        let host_resp = host
            .handle_message(client_hello, 1005, None)
            .unwrap()
            .expect("host resp");

        // Client handles host response
        let client_ack = client
            .handle_message(host_resp, 1010, None)
            .unwrap()
            .expect("client ack");

        // Host handles pair.done ack
        let host_close = host
            .handle_message(client_ack, 1015, None)
            .unwrap()
            .expect("host close");

        // Client handles close
        client.handle_message(host_close, 1020, None).unwrap();

        // Both are in Success state
        let mut client_store = TrustStore::new();
        let mut host_store = TrustStore::new();

        let peer_c = client.commit_trust(&mut client_store).unwrap();
        let peer_h = host.commit_trust(&mut host_store).unwrap();

        assert_eq!(peer_c.device_id, host_id);
        assert_eq!(peer_h.device_id, client_id);
        assert_eq!(client_store.len(), 1);
        assert_eq!(host_store.len(), 1);
    }

    #[test]
    fn test_qr_flow_forged_proof_rejected() {
        let secret = b"correct-secret".to_vec();
        let wrong_secret = b"wrong-secret".to_vec();

        let client_cfg = CeremonyConfig::qr_initiator("client-1", "Client", "host-1", wrong_secret);
        let host_cfg = CeremonyConfig::qr_responder("host-1", "Host", secret);

        let mut client = PairingCeremony::new(client_cfg, 1000);
        let mut host = PairingCeremony::new(host_cfg, 1000);

        let client_hello = client.start_initiation(None).unwrap().unwrap();
        let err = host.handle_message(client_hello, 1005, None).unwrap_err();
        assert_eq!(err, PairingError::ForgedProof);
    }

    #[test]
    fn test_qr_flow_replay_and_downgrade_rejected() {
        let secret = b"shared-pairing-secret".to_vec();
        let client_id = "client-replay";
        let host_id = "host-replay";

        // Setup host with persisted nonce counter = 10
        let host_cfg = CeremonyConfig::qr_responder_with_persisted_nonce(
            host_id,
            "Edge Hub",
            secret.clone(),
            10,
        );
        let mut host = PairingCeremony::new(host_cfg, 1000);

        // Attempt 1: Client tries replaying old nonce_counter = 5 (<= 10)
        let client_cfg_old = CeremonyConfig::qr_initiator_with_nonce(
            client_id,
            "Client App",
            host_id,
            secret.clone(),
            5,
        );
        let mut client_old = PairingCeremony::new(client_cfg_old, 1000);
        let replayed_hello = client_old.start_initiation(None).unwrap().unwrap();
        let err = host
            .handle_message(replayed_hello.clone(), 1005, None)
            .unwrap_err();
        assert_eq!(err, PairingError::ReplayDetected(5, 10));

        // Attempt 2: Attacker attempts protocol downgrade attack (version 0 < CURRENT_PROTOCOL_VERSION)
        let downgrade_hello = PairingMessage::QrClientHello {
            version: 0,
            client_id: client_id.to_string(),
            client_name: "Downgrade Attacker".to_string(),
            nonce_counter: 15,
            proof_hex: to_hex(&compute_pairing_proof(&secret, client_id, host_id, 15)),
        };
        let mut host2 = PairingCeremony::new(
            CeremonyConfig::qr_responder_with_persisted_nonce(
                host_id,
                "Edge Hub",
                secret.clone(),
                10,
            ),
            1000,
        );
        let downgrade_err = host2
            .handle_message(downgrade_hello, 1005, None)
            .unwrap_err();
        assert_eq!(downgrade_err, PairingError::DowngradeDetected(0));

        // Attempt 3: Valid monotonic nonce = 11 (> 10) succeeds on fresh host
        let mut host3 = PairingCeremony::new(
            CeremonyConfig::qr_responder_with_persisted_nonce(
                host_id,
                "Edge Hub",
                secret.clone(),
                10,
            ),
            1000,
        );
        let client_cfg_valid = CeremonyConfig::qr_initiator_with_nonce(
            client_id,
            "Client App",
            host_id,
            secret.clone(),
            11,
        );
        let mut client_valid = PairingCeremony::new(client_cfg_valid, 1000);
        let valid_hello = client_valid.start_initiation(None).unwrap().unwrap();
        let host_resp = host3
            .handle_message(valid_hello, 1005, None)
            .unwrap()
            .expect("host resp");

        let client_ack = client_valid
            .handle_message(host_resp, 1010, None)
            .unwrap()
            .expect("done ack");
        let close_resp = host3
            .handle_message(client_ack, 1015, None)
            .unwrap()
            .expect("close");
        client_valid.handle_message(close_resp, 1020, None).unwrap();

        let mut store = TrustStore::new();
        host3.commit_trust(&mut store).unwrap();
        assert_eq!(store.get_nonce_counter(client_id), 11);
    }

    #[test]
    fn test_sas_flow_happy_path() {
        let mut a = PairingCeremony::new(
            CeremonyConfig::numeric_compare("dev-a", "Device A", CeremonyRole::Initiator),
            1000,
        );
        let mut b = PairingCeremony::new(
            CeremonyConfig::numeric_compare("dev-b", "Device B", CeremonyRole::Responder),
            1000,
        );

        let na = vec![11u8; 16];
        let nb = vec![22u8; 16];

        let msg1 = a.start_initiation(Some(na)).unwrap().unwrap();
        let msg2 = b.handle_message(msg1, 1005, Some(nb)).unwrap().unwrap();
        let msg3 = a.handle_message(msg2, 1010, None).unwrap().unwrap();
        b.handle_message(msg3, 1015, None).unwrap();

        // Check computed SAS
        if let (
            CeremonyState::SasAwaitingUserConfirmation { sas: sas_a },
            CeremonyState::SasAwaitingUserConfirmation { sas: sas_b },
        ) = (a.state(), b.state())
        {
            assert_eq!(*sas_a, *sas_b);
        } else {
            panic!("states not awaiting sas confirmation");
        }

        // Both confirm
        let ack_a = a.confirm_sas(true, 1020).unwrap();
        let _ack_b = b.confirm_sas(true, 1020).unwrap();

        let close_b = b.handle_message(ack_a, 1025, None).unwrap().unwrap();
        a.handle_message(close_b, 1030, None).unwrap();

        let mut store_a = TrustStore::new();
        let mut store_b = TrustStore::new();
        a.commit_trust(&mut store_a).unwrap();
        b.commit_trust(&mut store_b).unwrap();
        assert_eq!(store_a.len(), 1);
        assert_eq!(store_b.len(), 1);
    }
}
