//! USB pairing flow via ADB host protocol.
//!
//! Provides:
//! - ADB message framing: `command`, `arg0`, `arg1`, `data_length`, `data_crc32`, `magic`
//! - Constants: `CNXN`, `OPEN`, `OKAY`, `CLSE`, `WRTE`
//! - ADB host client protocol state machine for connecting to the ADB daemon,
//!   issuing port forward / transport stream commands (e.g. `tcp:5555`),
//!   and multiplexing the pairing ceremony over the USB ADB transport.

use std::fmt;

/// ADB Command constants (little-endian 4-byte ASCII).
/// SYNC command.
pub const A_SYNC: u32 = 0x434e5953; // "SYNC"
/// CNXN connection handshake command.
pub const A_CNXN: u32 = 0x4e584e43; // "CNXN"
/// OPEN stream command.
pub const A_OPEN: u32 = 0x4e45504f; // "OPEN"
/// OKAY stream acknowledgement command.
pub const A_OKAY: u32 = 0x59414b4f; // "OKAY"
/// CLSE stream close command.
pub const A_CLSE: u32 = 0x45534c43; // "CLSE"
/// WRTE stream payload transfer command.
pub const A_WRTE: u32 = 0x45545257; // "WRTE"

/// Standard ADB protocol version.
pub const A_VERSION: u32 = 0x01000000;

/// Default maximum ADB payload size (typically 256KB or 4KB for minimal devices).
pub const MAX_ADB_PAYLOAD: u32 = 4096;

/// ADB Message Header (24 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdbMessageHeader {
    /// Command identifier (e.g., A_CNXN, A_OPEN, A_OKAY, A_CLSE, A_WRTE).
    pub command: u32,
    /// First command argument (e.g., local stream id).
    pub arg0: u32,
    /// Second command argument (e.g., remote stream id).
    pub arg1: u32,
    /// Length of payload in bytes following header.
    pub data_length: u32,
    /// Sum of all payload bytes mod 2^32 (CRC32/checksum in ADB wire format).
    pub data_crc32: u32,
    /// Integrity magic: `command ^ 0xFFFFFFFF`.
    pub magic: u32,
}

impl AdbMessageHeader {
    /// Create a new ADB message header with automatically computed magic.
    pub fn new(command: u32, arg0: u32, arg1: u32, payload: &[u8]) -> Self {
        let data_length = payload.len() as u32;
        let mut crc: u32 = 0;
        for &b in payload {
            crc = crc.wrapping_add(b as u32);
        }
        let magic = command ^ 0xFFFFFFFF;
        Self {
            command,
            arg0,
            arg1,
            data_length,
            data_crc32: crc,
            magic,
        }
    }

    /// Encode header into 24-byte little-endian array.
    pub fn to_bytes(&self) -> [u8; 24] {
        let mut buf = [0u8; 24];
        buf[0..4].copy_from_slice(&self.command.to_le_bytes());
        buf[4..8].copy_from_slice(&self.arg0.to_le_bytes());
        buf[8..12].copy_from_slice(&self.arg1.to_le_bytes());
        buf[12..16].copy_from_slice(&self.data_length.to_le_bytes());
        buf[16..20].copy_from_slice(&self.data_crc32.to_le_bytes());
        buf[20..24].copy_from_slice(&self.magic.to_le_bytes());
        buf
    }

    /// Parse a 24-byte slice into an ADB message header, verifying the magic field.
    pub fn parse(buf: &[u8; 24]) -> Result<Self, AdbError> {
        let command = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        let arg0 = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        let arg1 = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        let data_length = u32::from_le_bytes(buf[12..16].try_into().unwrap());
        let data_crc32 = u32::from_le_bytes(buf[16..20].try_into().unwrap());
        let magic = u32::from_le_bytes(buf[20..24].try_into().unwrap());

        if (command ^ 0xFFFFFFFF) != magic {
            return Err(AdbError::InvalidMagic);
        }

        Ok(Self {
            command,
            arg0,
            arg1,
            data_length,
            data_crc32,
            magic,
        })
    }
}

/// A complete ADB message consisting of a header and its payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbMessage {
    /// 24-byte message header.
    pub header: AdbMessageHeader,
    /// Associated payload bytes.
    pub payload: Vec<u8>,
}

impl AdbMessage {
    /// Create a new ADB message.
    pub fn new(command: u32, arg0: u32, arg1: u32, payload: Vec<u8>) -> Self {
        let header = AdbMessageHeader::new(command, arg0, arg1, &payload);
        Self { header, payload }
    }

    /// Encode the message into wire bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(24 + self.payload.len());
        out.extend_from_slice(&self.header.to_bytes());
        out.extend_from_slice(&self.payload);
        out
    }

    /// Verify payload checksum against header.
    pub fn verify_checksum(&self) -> bool {
        let mut crc: u32 = 0;
        for &b in &self.payload {
            crc = crc.wrapping_add(b as u32);
        }
        crc == self.header.data_crc32
    }
}

/// Error types for ADB host protocol operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdbError {
    /// Magic integrity check failed (`command ^ 0xFFFFFFFF != magic`).
    InvalidMagic,
    /// Checksum of payload did not match header.
    ChecksumMismatch,
    /// Incomplete message received.
    IncompleteMessage,
    /// Unexpected message received for current protocol state.
    UnexpectedCommand(u32),
    /// ADB channel was closed.
    ChannelClosed,
    /// Protocol error or invalid string.
    ProtocolError(String),
}

impl fmt::Display for AdbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => write!(f, "ADB header magic validation failed"),
            Self::ChecksumMismatch => write!(f, "ADB message payload checksum mismatch"),
            Self::IncompleteMessage => write!(f, "incomplete ADB message"),
            Self::UnexpectedCommand(cmd) => write!(f, "unexpected ADB command: 0x{cmd:08x}"),
            Self::ChannelClosed => write!(f, "ADB stream channel closed"),
            Self::ProtocolError(e) => write!(f, "ADB protocol error: {e}"),
        }
    }
}

impl std::error::Error for AdbError {}

/// ADB Host Stream session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdbSessionState {
    /// Initial disconnected state.
    Disconnected,
    /// Sent CNXN, waiting for device CNXN.
    Connecting,
    /// CNXN established, device ready.
    Connected,
    /// Sent OPEN for stream destination (e.g. `tcp:5555`), waiting for OKAY.
    OpeningStream,
    /// Stream channel active; bidirectional WRTE / OKAY enabled.
    StreamActive,
    /// Stream or connection closed.
    Closed,
}

/// ADB host protocol state handler for USB pairing.
#[derive(Debug, Clone)]
pub struct AdbHostSession {
    state: AdbSessionState,
    local_stream_id: u32,
    remote_stream_id: Option<u32>,
    system_identity: String,
}

impl AdbHostSession {
    /// Create a new ADB host session with a unique local stream id.
    pub fn new(local_stream_id: u32, system_identity: impl Into<String>) -> Self {
        Self {
            state: AdbSessionState::Disconnected,
            local_stream_id,
            remote_stream_id: None,
            system_identity: system_identity.into(),
        }
    }

    /// Current session state.
    pub fn state(&self) -> AdbSessionState {
        self.state
    }

    /// Initiate connection handshake (returns CNXN message).
    pub fn connect(&mut self) -> AdbMessage {
        self.state = AdbSessionState::Connecting;
        let id_bytes = format!("host::{}\0", self.system_identity).into_bytes();
        AdbMessage::new(A_CNXN, A_VERSION, MAX_ADB_PAYLOAD, id_bytes)
    }

    /// Request opening a service destination (e.g., `"tcp:5555\0"`).
    pub fn open_stream(&mut self, destination: &str) -> Result<AdbMessage, AdbError> {
        if self.state != AdbSessionState::Connected {
            return Err(AdbError::ProtocolError("not connected".into()));
        }
        self.state = AdbSessionState::OpeningStream;
        let mut dest_bytes = destination.as_bytes().to_vec();
        if !dest_bytes.ends_with(&[0]) {
            dest_bytes.push(0);
        }
        Ok(AdbMessage::new(A_OPEN, self.local_stream_id, 0, dest_bytes))
    }

    /// Package data payload into an ADB WRTE message over the active stream.
    pub fn write_payload(&self, data: Vec<u8>) -> Result<AdbMessage, AdbError> {
        if self.state != AdbSessionState::StreamActive {
            return Err(AdbError::ProtocolError("stream not active".into()));
        }
        let remote_id = self.remote_stream_id.ok_or(AdbError::ChannelClosed)?;
        Ok(AdbMessage::new(
            A_WRTE,
            self.local_stream_id,
            remote_id,
            data,
        ))
    }

    /// Create an OKAY acknowledgement message in response to a peer WRTE.
    pub fn ack_write(&self) -> Result<AdbMessage, AdbError> {
        let remote_id = self.remote_stream_id.ok_or(AdbError::ChannelClosed)?;
        Ok(AdbMessage::new(
            A_OKAY,
            self.local_stream_id,
            remote_id,
            Vec::new(),
        ))
    }

    /// Close the active stream.
    pub fn close_stream(&mut self) -> AdbMessage {
        let remote_id = self.remote_stream_id.unwrap_or(0);
        self.state = AdbSessionState::Closed;
        AdbMessage::new(A_CLSE, self.local_stream_id, remote_id, Vec::new())
    }

    /// Process an incoming ADB message from the device.
    pub fn handle_message(&mut self, msg: &AdbMessage) -> Result<Option<AdbMessage>, AdbError> {
        if !msg.verify_checksum() {
            return Err(AdbError::ChecksumMismatch);
        }

        match (self.state, msg.header.command) {
            (AdbSessionState::Connecting, A_CNXN) => {
                self.state = AdbSessionState::Connected;
                Ok(None)
            }
            (AdbSessionState::OpeningStream, A_OKAY) => {
                // Device acknowledged stream open: arg0 is remote_stream_id, arg1 is local_stream_id
                if msg.header.arg1 == self.local_stream_id {
                    self.remote_stream_id = Some(msg.header.arg0);
                    self.state = AdbSessionState::StreamActive;
                    Ok(None)
                } else {
                    Err(AdbError::ProtocolError("mismatched stream id".into()))
                }
            }
            (AdbSessionState::StreamActive, A_WRTE) => {
                // Incoming data from device over pairing stream; generate OKAY ack
                let ack = self.ack_write()?;
                Ok(Some(ack))
            }
            (AdbSessionState::StreamActive, A_OKAY) => {
                // Device acked our write
                Ok(None)
            }
            (_, A_CLSE) => {
                self.state = AdbSessionState::Closed;
                Ok(None)
            }
            _ => Err(AdbError::UnexpectedCommand(msg.header.command)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adb_header_serialize_and_parse() {
        let payload = b"hello adb payload";
        let header = AdbMessageHeader::new(A_CNXN, 1, 2, payload);
        let bytes = header.to_bytes();
        let parsed = AdbMessageHeader::parse(&bytes).expect("parse failed");
        assert_eq!(header, parsed);
        assert_eq!(parsed.command, A_CNXN);
        assert_eq!(parsed.data_length, payload.len() as u32);
    }

    #[test]
    fn test_adb_message_roundtrip() {
        let payload = b"cctv-pairing-data".to_vec();
        let msg = AdbMessage::new(A_WRTE, 10, 20, payload.clone());
        let wire = msg.to_bytes();

        let header_buf: [u8; 24] = wire[0..24].try_into().unwrap();
        let header = AdbMessageHeader::parse(&header_buf).unwrap();
        let read_payload = wire[24..].to_vec();

        let parsed_msg = AdbMessage {
            header,
            payload: read_payload,
        };
        assert!(parsed_msg.verify_checksum());
        assert_eq!(parsed_msg.payload, payload);
    }

    #[test]
    fn test_adb_session_handshake() {
        let mut host = AdbHostSession::new(1, "test-pc");
        assert_eq!(host.state(), AdbSessionState::Disconnected);

        // Host sends CNXN
        let cnxn_out = host.connect();
        assert_eq!(cnxn_out.header.command, A_CNXN);
        assert_eq!(host.state(), AdbSessionState::Connecting);

        // Device responds CNXN
        let dev_cnxn = AdbMessage::new(
            A_CNXN,
            A_VERSION,
            MAX_ADB_PAYLOAD,
            b"device::ro.product.model=Cam\0".to_vec(),
        );
        host.handle_message(&dev_cnxn).unwrap();
        assert_eq!(host.state(), AdbSessionState::Connected);

        // Host opens pairing stream
        let open_msg = host.open_stream("tcp:5555").unwrap();
        assert_eq!(open_msg.header.command, A_OPEN);
        assert_eq!(host.state(), AdbSessionState::OpeningStream);

        // Device responds OKAY (remote_stream_id = 42, local_stream_id = 1)
        let dev_okay = AdbMessage::new(A_OKAY, 42, 1, Vec::new());
        host.handle_message(&dev_okay).unwrap();
        assert_eq!(host.state(), AdbSessionState::StreamActive);

        // Host writes pairing message
        let write_msg = host
            .write_payload(b"{\"type\":\"QrClientHello\"}".to_vec())
            .unwrap();
        assert_eq!(write_msg.header.command, A_WRTE);
        assert_eq!(write_msg.header.arg0, 1);
        assert_eq!(write_msg.header.arg1, 42);
    }
}
