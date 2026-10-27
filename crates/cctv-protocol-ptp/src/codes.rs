//! ISO 15740 and PTP-IP operation, response, and packet type codes.

/// PTP-IP Packet types carried in the 8-byte PDU header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum PacketType {
    /// Init Command Request (0x00000001)
    InitCommandRequest = 0x0000_0001,
    /// Init Command Ack (0x00000002)
    InitCommandAck = 0x0000_0002,
    /// Init Event Request (0x00000003)
    InitEventRequest = 0x0000_0003,
    /// Init Event Ack (0x00000004)
    InitEventAck = 0x0000_0004,
    /// Init Fail (0x00000005)
    InitFail = 0x0000_0005,
    /// Operation Request (0x00000006)
    OperationRequest = 0x0000_0006,
    /// Operation Response (0x00000007)
    OperationResponse = 0x0000_0007,
    /// Event (0x00000008)
    Event = 0x0000_0008,
    /// Start Data (0x00000009)
    StartData = 0x0000_0009,
    /// Data Packet (0x0000000A)
    DataPacket = 0x0000_000A,
    /// Cancel (0x0000000B)
    Cancel = 0x0000_000B,
    /// End Data (0x0000000C)
    EndData = 0x0000_000C,
    /// Probe Request (0x0000000D)
    ProbeRequest = 0x0000_000D,
    /// Probe Response (0x0000000E)
    ProbeResponse = 0x0000_000E,
}

impl PacketType {
    /// Convert raw `u32` into `PacketType`, if known.
    pub fn from_u32(val: u32) -> Option<Self> {
        match val {
            0x0000_0001 => Some(Self::InitCommandRequest),
            0x0000_0002 => Some(Self::InitCommandAck),
            0x0000_0003 => Some(Self::InitEventRequest),
            0x0000_0004 => Some(Self::InitEventAck),
            0x0000_0005 => Some(Self::InitFail),
            0x0000_0006 => Some(Self::OperationRequest),
            0x0000_0007 => Some(Self::OperationResponse),
            0x0000_0008 => Some(Self::Event),
            0x0000_0009 => Some(Self::StartData),
            0x0000_000A => Some(Self::DataPacket),
            0x0000_000B => Some(Self::Cancel),
            0x0000_000C => Some(Self::EndData),
            0x0000_000D => Some(Self::ProbeRequest),
            0x0000_000E => Some(Self::ProbeResponse),
            _ => None,
        }
    }
}

/// PTP Operation Codes (ISO 15740).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum OperationCode {
    /// Get Device Info (0x1001)
    GetDeviceInfo = 0x1001,
    /// Open Session (0x1002)
    OpenSession = 0x1002,
    /// Close Session (0x1003)
    CloseSession = 0x1003,
    /// Get Storage IDs (0x1004)
    GetStorageIDs = 0x1004,
    /// Get Storage Info (0x1005)
    GetStorageInfo = 0x1005,
    /// Get Num Objects (0x1006)
    GetNumObjects = 0x1006,
    /// Get Object Handles (0x1007)
    GetObjectHandles = 0x1007,
    /// Get Object Info (0x1008)
    GetObjectInfo = 0x1008,
    /// Get Object (0x1009)
    GetObject = 0x1009,
    /// Get Thumb (0x100A)
    GetThumb = 0x100A,
    /// Delete Object (0x100B)
    DeleteObject = 0x100B,
    /// Send Object Info (0x100C)
    SendObjectInfo = 0x100C,
    /// Send Object (0x100D)
    SendObject = 0x100D,
    /// Initiate Capture (0x100E)
    InitiateCapture = 0x100E,
    /// Custom / Vendor specific code
    Unknown(u16),
}

impl OperationCode {
    /// Convert raw `u16` to `OperationCode`.
    pub fn from_u16(val: u16) -> Self {
        match val {
            0x1001 => Self::GetDeviceInfo,
            0x1002 => Self::OpenSession,
            0x1003 => Self::CloseSession,
            0x1004 => Self::GetStorageIDs,
            0x1005 => Self::GetStorageInfo,
            0x1006 => Self::GetNumObjects,
            0x1007 => Self::GetObjectHandles,
            0x1008 => Self::GetObjectInfo,
            0x1009 => Self::GetObject,
            0x100A => Self::GetThumb,
            0x100B => Self::DeleteObject,
            0x100C => Self::SendObjectInfo,
            0x100D => Self::SendObject,
            0x100E => Self::InitiateCapture,
            other => Self::Unknown(other),
        }
    }

    /// Convert `OperationCode` to `u16`.
    pub fn as_u16(self) -> u16 {
        match self {
            Self::GetDeviceInfo => 0x1001,
            Self::OpenSession => 0x1002,
            Self::CloseSession => 0x1003,
            Self::GetStorageIDs => 0x1004,
            Self::GetStorageInfo => 0x1005,
            Self::GetNumObjects => 0x1006,
            Self::GetObjectHandles => 0x1007,
            Self::GetObjectInfo => 0x1008,
            Self::GetObject => 0x1009,
            Self::GetThumb => 0x100A,
            Self::DeleteObject => 0x100B,
            Self::SendObjectInfo => 0x100C,
            Self::SendObject => 0x100D,
            Self::InitiateCapture => 0x100E,
            Self::Unknown(val) => val,
        }
    }
}

/// PTP Response Codes (ISO 15740).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum ResponseCode {
    /// OK (0x2001)
    Ok = 0x2001,
    /// General Error (0x2000)
    GeneralError = 0x2000,
    /// Session Not Open (0x2003)
    SessionNotOpen = 0x2003,
    /// Invalid Transaction ID (0x2004)
    InvalidTransactionID = 0x2004,
    /// Operation Not Supported (0x2005)
    OperationNotSupported = 0x2005,
    /// Parameter Not Supported (0x2006)
    ParameterNotSupported = 0x2006,
    /// Session Already Open (0x2008)
    SessionAlreadyOpen = 0x2008,
    /// Device Busy (0x2019)
    DeviceBusy = 0x2019,
    /// Unknown / Vendor specific code
    Unknown(u16),
}

impl ResponseCode {
    /// Convert raw `u16` to `ResponseCode`.
    pub fn from_u16(val: u16) -> Self {
        match val {
            0x2001 => Self::Ok,
            0x2000 => Self::GeneralError,
            0x2003 => Self::SessionNotOpen,
            0x2004 => Self::InvalidTransactionID,
            0x2005 => Self::OperationNotSupported,
            0x2006 => Self::ParameterNotSupported,
            0x2008 => Self::SessionAlreadyOpen,
            0x2019 => Self::DeviceBusy,
            other => Self::Unknown(other),
        }
    }

    /// Convert `ResponseCode` to `u16`.
    pub fn as_u16(self) -> u16 {
        match self {
            Self::Ok => 0x2001,
            Self::GeneralError => 0x2000,
            Self::SessionNotOpen => 0x2003,
            Self::InvalidTransactionID => 0x2004,
            Self::OperationNotSupported => 0x2005,
            Self::ParameterNotSupported => 0x2006,
            Self::SessionAlreadyOpen => 0x2008,
            Self::DeviceBusy => 0x2019,
            Self::Unknown(val) => val,
        }
    }
}

/// Data phase directional indicator for Operation Request PDUs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum DataPhaseInfo {
    /// No data phase will follow (0x00000000)
    NoData = 0,
    /// Data will be transferred from initiator to responder (0x00000001)
    DataOut = 1,
    /// Data will be transferred from responder to initiator (0x00000002)
    DataIn = 2,
}

impl DataPhaseInfo {
    /// Convert raw `u32` to `DataPhaseInfo`.
    pub fn from_u32(val: u32) -> Self {
        match val {
            1 => Self::DataOut,
            2 => Self::DataIn,
            _ => Self::NoData,
        }
    }
}
