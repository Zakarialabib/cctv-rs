//! PTP-IP Packet Data Units (PDUs) and serialization codecs.

use crate::codes::{DataPhaseInfo, OperationCode, PacketType, ResponseCode};
use crate::error::PtpError;

/// PTP Device Info dataset (ISO 15740).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Standard version.
    pub standard_version: u16,
    /// Vendor extension ID.
    pub vendor_extension_id: u32,
    /// Vendor extension version.
    pub vendor_extension_version: u16,
    /// Vendor extension description.
    pub vendor_extension_desc: String,
    /// Functional mode.
    pub functional_mode: u16,
    /// Supported operation codes.
    pub operations_supported: Vec<u16>,
    /// Supported event codes.
    pub events_supported: Vec<u16>,
    /// Supported device property codes.
    pub device_properties_supported: Vec<u16>,
    /// Supported capture formats.
    pub capture_formats: Vec<u16>,
    /// Supported image formats.
    pub image_formats: Vec<u16>,
    /// Manufacturer name.
    pub manufacturer: String,
    /// Model name.
    pub model: String,
    /// Device version / firmware.
    pub device_version: String,
    /// Serial number.
    pub serial_number: String,
}

impl DeviceInfo {
    /// Encode `DeviceInfo` dataset into PTP binary format.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&self.standard_version.to_le_bytes());
        buf.extend_from_slice(&self.vendor_extension_id.to_le_bytes());
        buf.extend_from_slice(&self.vendor_extension_version.to_le_bytes());
        encode_ptp_string(&mut buf, &self.vendor_extension_desc);
        buf.extend_from_slice(&self.functional_mode.to_le_bytes());

        encode_u16_array(&mut buf, &self.operations_supported);
        encode_u16_array(&mut buf, &self.events_supported);
        encode_u16_array(&mut buf, &self.device_properties_supported);
        encode_u16_array(&mut buf, &self.capture_formats);
        encode_u16_array(&mut buf, &self.image_formats);

        encode_ptp_string(&mut buf, &self.manufacturer);
        encode_ptp_string(&mut buf, &self.model);
        encode_ptp_string(&mut buf, &self.device_version);
        encode_ptp_string(&mut buf, &self.serial_number);
        buf
    }

    /// Decode `DeviceInfo` dataset from PTP binary format.
    pub fn decode(bytes: &[u8]) -> Result<Self, PtpError> {
        let mut cursor = 0;

        let standard_version = read_u16(bytes, &mut cursor)?;
        let vendor_extension_id = read_u32(bytes, &mut cursor)?;
        let vendor_extension_version = read_u16(bytes, &mut cursor)?;
        let vendor_extension_desc = decode_ptp_string(bytes, &mut cursor)?;
        let functional_mode = read_u16(bytes, &mut cursor)?;

        let operations_supported = decode_u16_array(bytes, &mut cursor)?;
        let events_supported = decode_u16_array(bytes, &mut cursor)?;
        let device_properties_supported = decode_u16_array(bytes, &mut cursor)?;
        let capture_formats = decode_u16_array(bytes, &mut cursor)?;
        let image_formats = decode_u16_array(bytes, &mut cursor)?;

        let manufacturer = decode_ptp_string(bytes, &mut cursor)?;
        let model = decode_ptp_string(bytes, &mut cursor)?;
        let device_version = decode_ptp_string(bytes, &mut cursor)?;
        let serial_number = decode_ptp_string(bytes, &mut cursor)?;

        Ok(Self {
            standard_version,
            vendor_extension_id,
            vendor_extension_version,
            vendor_extension_desc,
            functional_mode,
            operations_supported,
            events_supported,
            device_properties_supported,
            capture_formats,
            image_formats,
            manufacturer,
            model,
            device_version,
            serial_number,
        })
    }
}

/// Init Command Request PDU payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitCommandRequestPdu {
    /// GUID identifying the initiator.
    pub guid: [u8; 16],
    /// Friendly host name.
    pub name: String,
    /// PTP-IP Protocol Version (e.g. 0x00010000).
    pub version: u32,
}

/// Init Command Ack PDU payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitCommandAckPdu {
    /// Connection number assigned by responder.
    pub connection_number: u32,
    /// GUID of the responder.
    pub guid: [u8; 16],
    /// Friendly responder name.
    pub responder_name: String,
    /// Protocol version.
    pub version: u32,
}

/// Operation Request PDU payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRequestPdu {
    /// Direction of following data phase.
    pub data_phase: DataPhaseInfo,
    /// Operation Code.
    pub operation_code: OperationCode,
    /// Transaction ID.
    pub transaction_id: u32,
    /// Up to 5 operation parameters.
    pub params: Vec<u32>,
}

/// Operation Response PDU payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationResponsePdu {
    /// Response Code.
    pub response_code: ResponseCode,
    /// Transaction ID matching request.
    pub transaction_id: u32,
    /// Response parameters.
    pub params: Vec<u32>,
}

/// Start Data PDU payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartDataPdu {
    /// Transaction ID.
    pub transaction_id: u32,
    /// Total byte length of upcoming data phase.
    pub total_length: u64,
}

/// Data Packet PDU payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPacketPdu {
    /// Transaction ID.
    pub transaction_id: u32,
    /// Data payload chunk.
    pub payload: Vec<u8>,
}

/// End Data PDU payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndDataPdu {
    /// Transaction ID.
    pub transaction_id: u32,
    /// Final data payload chunk.
    pub payload: Vec<u8>,
}

/// A parsed PTP-IP Packet Data Unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pdu {
    /// Init Command Request
    InitCommandRequest(InitCommandRequestPdu),
    /// Init Command Ack
    InitCommandAck(InitCommandAckPdu),
    /// Operation Request
    OperationRequest(OperationRequestPdu),
    /// Operation Response
    OperationResponse(OperationResponsePdu),
    /// Start Data
    StartData(StartDataPdu),
    /// Data Packet
    DataPacket(DataPacketPdu),
    /// End Data
    EndData(EndDataPdu),
}

impl Pdu {
    /// Encode PDU into binary byte vector for wire transmission.
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::new();
        let packet_type = match self {
            Self::InitCommandRequest(pdu) => {
                body.extend_from_slice(&pdu.guid);
                encode_null_utf16le(&mut body, &pdu.name);
                body.extend_from_slice(&pdu.version.to_le_bytes());
                PacketType::InitCommandRequest
            }
            Self::InitCommandAck(pdu) => {
                body.extend_from_slice(&pdu.connection_number.to_le_bytes());
                body.extend_from_slice(&pdu.guid);
                encode_null_utf16le(&mut body, &pdu.responder_name);
                body.extend_from_slice(&pdu.version.to_le_bytes());
                PacketType::InitCommandAck
            }
            Self::OperationRequest(pdu) => {
                body.extend_from_slice(&(pdu.data_phase as u32).to_le_bytes());
                body.extend_from_slice(&pdu.operation_code.as_u16().to_le_bytes());
                body.extend_from_slice(&pdu.transaction_id.to_le_bytes());
                for param in &pdu.params {
                    body.extend_from_slice(&param.to_le_bytes());
                }
                PacketType::OperationRequest
            }
            Self::OperationResponse(pdu) => {
                body.extend_from_slice(&pdu.response_code.as_u16().to_le_bytes());
                body.extend_from_slice(&pdu.transaction_id.to_le_bytes());
                for param in &pdu.params {
                    body.extend_from_slice(&param.to_le_bytes());
                }
                PacketType::OperationResponse
            }
            Self::StartData(pdu) => {
                body.extend_from_slice(&pdu.transaction_id.to_le_bytes());
                body.extend_from_slice(&pdu.total_length.to_le_bytes());
                PacketType::StartData
            }
            Self::DataPacket(pdu) => {
                body.extend_from_slice(&pdu.transaction_id.to_le_bytes());
                body.extend_from_slice(&pdu.payload);
                PacketType::DataPacket
            }
            Self::EndData(pdu) => {
                body.extend_from_slice(&pdu.transaction_id.to_le_bytes());
                body.extend_from_slice(&pdu.payload);
                PacketType::EndData
            }
        };

        let total_len = (8 + body.len()) as u32;
        let mut packet = Vec::with_capacity(total_len as usize);
        packet.extend_from_slice(&total_len.to_le_bytes());
        packet.extend_from_slice(&(packet_type as u32).to_le_bytes());
        packet.extend_from_slice(&body);
        packet
    }

    /// Decode PDU from binary buffer.
    pub fn decode(bytes: &[u8]) -> Result<Self, PtpError> {
        if bytes.len() < 8 {
            return Err(PtpError::MalformedPdu("Header under 8 bytes".into()));
        }
        let length = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
        if bytes.len() < length {
            return Err(PtpError::MalformedPdu("Incomplete PDU buffer".into()));
        }

        let raw_type = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let packet_type =
            PacketType::from_u32(raw_type).ok_or(PtpError::UnexpectedPacketType(raw_type))?;

        let body = &bytes[8..length];
        let mut cursor = 0;

        match packet_type {
            PacketType::InitCommandRequest => {
                if body.len() < 16 + 2 + 4 {
                    return Err(PtpError::MalformedPdu(
                        "InitCommandRequest too short".into(),
                    ));
                }
                let mut guid = [0u8; 16];
                guid.copy_from_slice(&body[0..16]);
                cursor = 16;
                let name = decode_null_utf16le(body, &mut cursor)?;
                let version = read_u32(body, &mut cursor)?;
                Ok(Self::InitCommandRequest(InitCommandRequestPdu {
                    guid,
                    name,
                    version,
                }))
            }
            PacketType::InitCommandAck => {
                let connection_number = read_u32(body, &mut cursor)?;
                if body.len() < cursor + 16 {
                    return Err(PtpError::MalformedPdu("InitCommandAck missing GUID".into()));
                }
                let mut guid = [0u8; 16];
                guid.copy_from_slice(&body[cursor..cursor + 16]);
                cursor += 16;
                let responder_name = decode_null_utf16le(body, &mut cursor)?;
                let version = read_u32(body, &mut cursor)?;
                Ok(Self::InitCommandAck(InitCommandAckPdu {
                    connection_number,
                    guid,
                    responder_name,
                    version,
                }))
            }
            PacketType::OperationRequest => {
                let data_phase_raw = read_u32(body, &mut cursor)?;
                let data_phase = DataPhaseInfo::from_u32(data_phase_raw);
                let op_code = read_u16(body, &mut cursor)?;
                let operation_code = OperationCode::from_u16(op_code);
                let transaction_id = read_u32(body, &mut cursor)?;
                let mut params = Vec::new();
                while cursor + 4 <= body.len() {
                    params.push(read_u32(body, &mut cursor)?);
                }
                Ok(Self::OperationRequest(OperationRequestPdu {
                    data_phase,
                    operation_code,
                    transaction_id,
                    params,
                }))
            }
            PacketType::OperationResponse => {
                let resp_code = read_u16(body, &mut cursor)?;
                let response_code = ResponseCode::from_u16(resp_code);
                let transaction_id = read_u32(body, &mut cursor)?;
                let mut params = Vec::new();
                while cursor + 4 <= body.len() {
                    params.push(read_u32(body, &mut cursor)?);
                }
                Ok(Self::OperationResponse(OperationResponsePdu {
                    response_code,
                    transaction_id,
                    params,
                }))
            }
            PacketType::StartData => {
                let transaction_id = read_u32(body, &mut cursor)?;
                let total_length = read_u64(body, &mut cursor)?;
                Ok(Self::StartData(StartDataPdu {
                    transaction_id,
                    total_length,
                }))
            }
            PacketType::DataPacket => {
                let transaction_id = read_u32(body, &mut cursor)?;
                let payload = body[cursor..].to_vec();
                Ok(Self::DataPacket(DataPacketPdu {
                    transaction_id,
                    payload,
                }))
            }
            PacketType::EndData => {
                let transaction_id = read_u32(body, &mut cursor)?;
                let payload = body[cursor..].to_vec();
                Ok(Self::EndData(EndDataPdu {
                    transaction_id,
                    payload,
                }))
            }
            _ => Err(PtpError::UnexpectedPacketType(raw_type)),
        }
    }
}

fn read_u16(slice: &[u8], cursor: &mut usize) -> Result<u16, PtpError> {
    if *cursor + 2 > slice.len() {
        return Err(PtpError::MalformedPdu("Truncated u16".into()));
    }
    let val = u16::from_le_bytes(slice[*cursor..*cursor + 2].try_into().unwrap());
    *cursor += 2;
    Ok(val)
}

fn read_u32(slice: &[u8], cursor: &mut usize) -> Result<u32, PtpError> {
    if *cursor + 4 > slice.len() {
        return Err(PtpError::MalformedPdu("Truncated u32".into()));
    }
    let val = u32::from_le_bytes(slice[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;
    Ok(val)
}

fn read_u64(slice: &[u8], cursor: &mut usize) -> Result<u64, PtpError> {
    if *cursor + 8 > slice.len() {
        return Err(PtpError::MalformedPdu("Truncated u64".into()));
    }
    let val = u64::from_le_bytes(slice[*cursor..*cursor + 8].try_into().unwrap());
    *cursor += 8;
    Ok(val)
}

fn encode_null_utf16le(buf: &mut Vec<u8>, text: &str) {
    for u in text.encode_utf16() {
        buf.extend_from_slice(&u.to_le_bytes());
    }
    buf.extend_from_slice(&[0, 0]);
}

fn decode_null_utf16le(slice: &[u8], cursor: &mut usize) -> Result<String, PtpError> {
    let mut units = Vec::new();
    while *cursor + 2 <= slice.len() {
        let code = u16::from_le_bytes(slice[*cursor..*cursor + 2].try_into().unwrap());
        *cursor += 2;
        if code == 0 {
            break;
        }
        units.push(code);
    }
    String::from_utf16(&units).map_err(|_| PtpError::MalformedPdu("Invalid UTF-16 text".into()))
}

fn encode_ptp_string(buf: &mut Vec<u8>, text: &str) {
    let units: Vec<u16> = text.encode_utf16().collect();
    let num_chars = (units.len() + 1) as u8;
    buf.push(num_chars);
    for u in units {
        buf.extend_from_slice(&u.to_le_bytes());
    }
    buf.extend_from_slice(&[0, 0]);
}

fn decode_ptp_string(slice: &[u8], cursor: &mut usize) -> Result<String, PtpError> {
    if *cursor >= slice.len() {
        return Err(PtpError::MalformedPdu("Missing PTP string len".into()));
    }
    let count = slice[*cursor] as usize;
    *cursor += 1;
    if count == 0 {
        return Ok(String::new());
    }
    let mut units = Vec::new();
    for _ in 0..count.saturating_sub(1) {
        if *cursor + 2 > slice.len() {
            return Err(PtpError::MalformedPdu("Truncated PTP string".into()));
        }
        let code = u16::from_le_bytes(slice[*cursor..*cursor + 2].try_into().unwrap());
        *cursor += 2;
        units.push(code);
    }
    // Skip trailing null word if present
    if *cursor + 2 <= slice.len() {
        let code = u16::from_le_bytes(slice[*cursor..*cursor + 2].try_into().unwrap());
        if code == 0 {
            *cursor += 2;
        }
    }
    String::from_utf16(&units).map_err(|_| PtpError::MalformedPdu("Invalid UTF-16 string".into()))
}

fn encode_u16_array(buf: &mut Vec<u8>, arr: &[u16]) {
    buf.extend_from_slice(&(arr.len() as u32).to_le_bytes());
    for item in arr {
        buf.extend_from_slice(&item.to_le_bytes());
    }
}

fn decode_u16_array(slice: &[u8], cursor: &mut usize) -> Result<Vec<u16>, PtpError> {
    let count = read_u32(slice, cursor)? as usize;
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(read_u16(slice, cursor)?);
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdu_encode_decode_roundtrip() {
        let req = Pdu::InitCommandRequest(InitCommandRequestPdu {
            guid: [0x42; 16],
            name: "TestHost".into(),
            version: 0x0001_0000,
        });

        let encoded = req.encode();
        let decoded = Pdu::decode(&encoded).expect("Decode should succeed");
        assert_eq!(req, decoded);
    }

    #[test]
    fn test_device_info_encode_decode_roundtrip() {
        let info = DeviceInfo {
            standard_version: 100,
            vendor_extension_id: 1,
            vendor_extension_version: 100,
            vendor_extension_desc: "Desc".into(),
            functional_mode: 0,
            operations_supported: vec![0x1001, 0x1002],
            events_supported: vec![0x4001],
            device_properties_supported: vec![0x5001],
            capture_formats: vec![0x3801],
            image_formats: vec![0x3801],
            manufacturer: "Test Vendor".into(),
            model: "Model X".into(),
            device_version: "1.0".into(),
            serial_number: "SN-999".into(),
        };

        let encoded = info.encode();
        let decoded = DeviceInfo::decode(&encoded).expect("DeviceInfo decode");
        assert_eq!(info, decoded);
    }
}
