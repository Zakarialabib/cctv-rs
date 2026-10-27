//! Async PTP-IP client implementation.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::codes::{DataPhaseInfo, OperationCode, ResponseCode};
use crate::error::PtpError;
use crate::pdu::{DeviceInfo, InitCommandAckPdu, InitCommandRequestPdu, OperationRequestPdu, Pdu};

/// Async PTP-IP client for camera management and object retrieval.
pub struct PtpClient {
    stream: TcpStream,
    transaction_id: u32,
    session_id: Option<u32>,
}

impl PtpClient {
    /// Create client from an connected TCP stream.
    pub fn from_stream(stream: TcpStream) -> Self {
        Self {
            stream,
            transaction_id: 1,
            session_id: None,
        }
    }

    /// Connect to a remote PTP-IP responder address.
    pub async fn connect(addr: &str) -> Result<Self, PtpError> {
        let stream = TcpStream::connect(addr).await?;
        Ok(Self::from_stream(stream))
    }

    /// Initialize PTP-IP connection handshake.
    pub async fn init_session(
        &mut self,
        guid: [u8; 16],
        name: &str,
    ) -> Result<InitCommandAckPdu, PtpError> {
        let req = Pdu::InitCommandRequest(InitCommandRequestPdu {
            guid,
            name: name.to_string(),
            version: 0x0001_0000,
        });
        self.send_pdu(&req).await?;

        let response = self.read_pdu().await?;
        match response {
            Pdu::InitCommandAck(ack) => Ok(ack),
            other => Err(PtpError::MalformedPdu(format!(
                "Expected InitCommandAck, got {other:?}"
            ))),
        }
    }

    /// Open a PTP session with the specified session ID.
    pub async fn open_session(&mut self, session_id: u32) -> Result<(), PtpError> {
        if self.session_id.is_some() {
            return Err(PtpError::SessionAlreadyOpen);
        }
        let tx_id = self.next_transaction_id();
        let req = Pdu::OperationRequest(OperationRequestPdu {
            data_phase: DataPhaseInfo::NoData,
            operation_code: OperationCode::OpenSession,
            transaction_id: tx_id,
            params: vec![session_id],
        });
        self.send_pdu(&req).await?;

        let resp = self.read_operation_response(tx_id).await?;
        if resp.response_code == ResponseCode::Ok {
            self.session_id = Some(session_id);
            Ok(())
        } else {
            Err(PtpError::ResponseError(resp.response_code))
        }
    }

    /// Request and parse PTP `DeviceInfo`.
    pub async fn get_device_info(&mut self) -> Result<DeviceInfo, PtpError> {
        self.ensure_session()?;
        let tx_id = self.next_transaction_id();
        let req = Pdu::OperationRequest(OperationRequestPdu {
            data_phase: DataPhaseInfo::DataIn,
            operation_code: OperationCode::GetDeviceInfo,
            transaction_id: tx_id,
            params: Vec::new(),
        });
        self.send_pdu(&req).await?;

        let payload = self.read_data_phase(tx_id).await?;
        let resp = self.read_operation_response(tx_id).await?;
        if resp.response_code != ResponseCode::Ok {
            return Err(PtpError::ResponseError(resp.response_code));
        }

        DeviceInfo::decode(&payload)
    }

    /// Retrieve list of storage IDs available on responder.
    pub async fn get_storage_ids(&mut self) -> Result<Vec<u32>, PtpError> {
        self.ensure_session()?;
        let tx_id = self.next_transaction_id();
        let req = Pdu::OperationRequest(OperationRequestPdu {
            data_phase: DataPhaseInfo::DataIn,
            operation_code: OperationCode::GetStorageIDs,
            transaction_id: tx_id,
            params: Vec::new(),
        });
        self.send_pdu(&req).await?;

        let payload = self.read_data_phase(tx_id).await?;
        let resp = self.read_operation_response(tx_id).await?;
        if resp.response_code != ResponseCode::Ok {
            return Err(PtpError::ResponseError(resp.response_code));
        }

        if payload.len() < 4 {
            return Err(PtpError::MalformedPdu("Storage IDs missing count".into()));
        }
        let count = u32::from_le_bytes(payload[0..4].try_into().unwrap()) as usize;
        let mut ids = Vec::with_capacity(count);
        let mut cursor = 4;
        for _ in 0..count {
            if cursor + 4 > payload.len() {
                break;
            }
            ids.push(u32::from_le_bytes(
                payload[cursor..cursor + 4].try_into().unwrap(),
            ));
            cursor += 4;
        }
        Ok(ids)
    }

    /// Close active PTP session.
    pub async fn close_session(&mut self) -> Result<(), PtpError> {
        self.ensure_session()?;
        let tx_id = self.next_transaction_id();
        let req = Pdu::OperationRequest(OperationRequestPdu {
            data_phase: DataPhaseInfo::NoData,
            operation_code: OperationCode::CloseSession,
            transaction_id: tx_id,
            params: Vec::new(),
        });
        self.send_pdu(&req).await?;

        let resp = self.read_operation_response(tx_id).await?;
        self.session_id = None;
        if resp.response_code == ResponseCode::Ok {
            Ok(())
        } else {
            Err(PtpError::ResponseError(resp.response_code))
        }
    }

    fn ensure_session(&self) -> Result<(), PtpError> {
        if self.session_id.is_none() {
            Err(PtpError::SessionNotOpen)
        } else {
            Ok(())
        }
    }

    fn next_transaction_id(&mut self) -> u32 {
        let id = self.transaction_id;
        self.transaction_id += 1;
        id
    }

    async fn send_pdu(&mut self, pdu: &Pdu) -> Result<(), PtpError> {
        let bytes = pdu.encode();
        self.stream.write_all(&bytes).await?;
        Ok(())
    }

    async fn read_pdu(&mut self) -> Result<Pdu, PtpError> {
        let mut header = [0u8; 8];
        self.stream.read_exact(&mut header).await?;
        let length = u32::from_le_bytes(header[0..4].try_into().unwrap()) as usize;
        if length < 8 {
            return Err(PtpError::MalformedPdu("PDU length < 8".into()));
        }

        let mut buf = vec![0u8; length];
        buf[0..8].copy_from_slice(&header);
        self.stream.read_exact(&mut buf[8..length]).await?;

        Pdu::decode(&buf)
    }

    async fn read_data_phase(&mut self, tx_id: u32) -> Result<Vec<u8>, PtpError> {
        let mut data = Vec::new();

        loop {
            let pdu = self.read_pdu().await?;
            match pdu {
                Pdu::StartData(start) => {
                    if start.transaction_id != tx_id {
                        return Err(PtpError::TransactionMismatch {
                            expected: tx_id,
                            got: start.transaction_id,
                        });
                    }
                }
                Pdu::DataPacket(pkt) => {
                    if pkt.transaction_id != tx_id {
                        return Err(PtpError::TransactionMismatch {
                            expected: tx_id,
                            got: pkt.transaction_id,
                        });
                    }
                    data.extend_from_slice(&pkt.payload);
                }
                Pdu::EndData(end) => {
                    if end.transaction_id != tx_id {
                        return Err(PtpError::TransactionMismatch {
                            expected: tx_id,
                            got: end.transaction_id,
                        });
                    }
                    data.extend_from_slice(&end.payload);
                    break;
                }
                other => {
                    return Err(PtpError::MalformedPdu(format!(
                        "Expected Data PDU, got {other:?}"
                    )))
                }
            }
        }

        Ok(data)
    }

    async fn read_operation_response(
        &mut self,
        tx_id: u32,
    ) -> Result<crate::pdu::OperationResponsePdu, PtpError> {
        let pdu = self.read_pdu().await?;
        match pdu {
            Pdu::OperationResponse(resp) => {
                if resp.transaction_id != tx_id {
                    Err(PtpError::TransactionMismatch {
                        expected: tx_id,
                        got: resp.transaction_id,
                    })
                } else {
                    Ok(resp)
                }
            }
            other => Err(PtpError::MalformedPdu(format!(
                "Expected OperationResponse, got {other:?}"
            ))),
        }
    }
}
