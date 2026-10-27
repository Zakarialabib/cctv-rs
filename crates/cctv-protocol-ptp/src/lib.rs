//! Picture Transfer Protocol (PTP / ISO 15740 / PTP-IP) implementation for cctv-rs.
//!
//! Provides PDU codecs, operation codes, session management client, and mock responder.

#![forbid(unsafe_code)]

pub mod client;
pub mod codes;
pub mod error;
pub mod mock;
pub mod pdu;

pub use client::PtpClient;
pub use codes::{DataPhaseInfo, OperationCode, PacketType, ResponseCode};
pub use error::PtpError;
pub use mock::MockPtpResponder;
pub use pdu::{
    DataPacketPdu, DeviceInfo, EndDataPdu, InitCommandAckPdu, InitCommandRequestPdu,
    OperationRequestPdu, OperationResponsePdu, Pdu, StartDataPdu,
};
