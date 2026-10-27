//! Error types for PTP / PTP-IP operations.

use thiserror::Error;

use crate::codes::ResponseCode;

/// Errors that can occur during PTP wire operations or session handling.
#[derive(Debug, Error)]
pub enum PtpError {
    /// An I/O error occurred during socket operations.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A PDU received over the wire was malformed or truncated.
    #[error("Malformed PDU: {0}")]
    MalformedPdu(String),

    /// An unexpected PDU packet type was received.
    #[error("Unexpected PDU packet type: {0:#010x}")]
    UnexpectedPacketType(u32),

    /// A transaction ID mismatch occurred between request and response.
    #[error("Transaction ID mismatch: expected {expected}, got {got}")]
    TransactionMismatch {
        /// The expected transaction ID.
        expected: u32,
        /// The transaction ID received in response.
        got: u32,
    },

    /// The PTP responder returned an error response code.
    #[error("PTP response error code: {0:?}")]
    ResponseError(ResponseCode),

    /// Tried to open a session when one was already open.
    #[error("PTP session already open")]
    SessionAlreadyOpen,

    /// Executed an operation without an active session.
    #[error("PTP session not open")]
    SessionNotOpen,

    /// Connection was closed prematurely.
    #[error("PTP connection closed")]
    Closed,
}
