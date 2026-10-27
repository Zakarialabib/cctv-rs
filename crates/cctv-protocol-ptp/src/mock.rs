//! Mock PTP Responder for round-trip unit and integration testing.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::codes::{OperationCode, ResponseCode};
use crate::error::PtpError;
use crate::pdu::{
    DeviceInfo, EndDataPdu, InitCommandAckPdu, OperationResponsePdu, Pdu, StartDataPdu,
};

/// Mock PTP Responder server to emulate a physical camera for round-trip testing.
pub struct MockPtpResponder {
    listener: TcpListener,
    device_info: DeviceInfo,
    storage_ids: Vec<u32>,
}

impl MockPtpResponder {
    /// Bind a mock responder server to loopback port `0`.
    pub async fn bind() -> Result<Self, PtpError> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let device_info = DeviceInfo {
            standard_version: 100,
            vendor_extension_id: 0,
            vendor_extension_version: 100,
            vendor_extension_desc: "Standard PTP".to_string(),
            functional_mode: 0,
            operations_supported: vec![0x1001, 0x1002, 0x1003, 0x1004],
            events_supported: Vec::new(),
            device_properties_supported: Vec::new(),
            capture_formats: vec![0x3801],
            image_formats: vec![0x3801],
            manufacturer: "CCTV Mock".to_string(),
            model: "Pro-Cam 9000".to_string(),
            device_version: "v1.0.0".to_string(),
            serial_number: "MOCK-12345".to_string(),
        };
        let storage_ids = vec![0x0001_0001];

        Ok(Self {
            listener,
            device_info,
            storage_ids,
        })
    }

    /// Return the bound socket address string.
    pub fn local_addr(&self) -> String {
        self.listener.local_addr().unwrap().to_string()
    }

    /// Run the mock responder server loop processing client requests until disconnected.
    pub async fn run(self) -> Result<(), PtpError> {
        let (mut stream, _) = self.listener.accept().await?;
        let mut session_active = false;

        loop {
            let mut header = [0u8; 8];
            if stream.read_exact(&mut header).await.is_err() {
                break; // Connection closed
            }
            let length = u32::from_le_bytes(header[0..4].try_into().unwrap()) as usize;
            if length < 8 {
                break;
            }
            let mut buf = vec![0u8; length];
            buf[0..8].copy_from_slice(&header);
            if stream.read_exact(&mut buf[8..length]).await.is_err() {
                break;
            }

            let pdu = Pdu::decode(&buf)?;
            match pdu {
                Pdu::InitCommandRequest(_req) => {
                    let ack = Pdu::InitCommandAck(InitCommandAckPdu {
                        connection_number: 1,
                        guid: [0u8; 16],
                        responder_name: "Mock-PTP".into(),
                        version: 0x0001_0000,
                    });
                    stream.write_all(&ack.encode()).await?;
                }
                Pdu::OperationRequest(op) => match op.operation_code {
                    OperationCode::OpenSession => {
                        session_active = true;
                        let resp = Pdu::OperationResponse(OperationResponsePdu {
                            response_code: ResponseCode::Ok,
                            transaction_id: op.transaction_id,
                            params: Vec::new(),
                        });
                        stream.write_all(&resp.encode()).await?;
                    }
                    OperationCode::CloseSession => {
                        session_active = false;
                        let resp = Pdu::OperationResponse(OperationResponsePdu {
                            response_code: ResponseCode::Ok,
                            transaction_id: op.transaction_id,
                            params: Vec::new(),
                        });
                        stream.write_all(&resp.encode()).await?;
                    }
                    OperationCode::GetDeviceInfo => {
                        if !session_active {
                            let resp = Pdu::OperationResponse(OperationResponsePdu {
                                response_code: ResponseCode::SessionNotOpen,
                                transaction_id: op.transaction_id,
                                params: Vec::new(),
                            });
                            stream.write_all(&resp.encode()).await?;
                            continue;
                        }
                        let payload = self.device_info.encode();
                        let start = Pdu::StartData(StartDataPdu {
                            transaction_id: op.transaction_id,
                            total_length: payload.len() as u64,
                        });
                        let end = Pdu::EndData(EndDataPdu {
                            transaction_id: op.transaction_id,
                            payload,
                        });
                        let resp = Pdu::OperationResponse(OperationResponsePdu {
                            response_code: ResponseCode::Ok,
                            transaction_id: op.transaction_id,
                            params: Vec::new(),
                        });
                        stream.write_all(&start.encode()).await?;
                        stream.write_all(&end.encode()).await?;
                        stream.write_all(&resp.encode()).await?;
                    }
                    OperationCode::GetStorageIDs => {
                        if !session_active {
                            let resp = Pdu::OperationResponse(OperationResponsePdu {
                                response_code: ResponseCode::SessionNotOpen,
                                transaction_id: op.transaction_id,
                                params: Vec::new(),
                            });
                            stream.write_all(&resp.encode()).await?;
                            continue;
                        }
                        let mut payload = Vec::new();
                        payload.extend_from_slice(&(self.storage_ids.len() as u32).to_le_bytes());
                        for id in &self.storage_ids {
                            payload.extend_from_slice(&id.to_le_bytes());
                        }

                        let start = Pdu::StartData(StartDataPdu {
                            transaction_id: op.transaction_id,
                            total_length: payload.len() as u64,
                        });
                        let end = Pdu::EndData(EndDataPdu {
                            transaction_id: op.transaction_id,
                            payload,
                        });
                        let resp = Pdu::OperationResponse(OperationResponsePdu {
                            response_code: ResponseCode::Ok,
                            transaction_id: op.transaction_id,
                            params: Vec::new(),
                        });
                        stream.write_all(&start.encode()).await?;
                        stream.write_all(&end.encode()).await?;
                        stream.write_all(&resp.encode()).await?;
                    }
                    _ => {
                        let resp = Pdu::OperationResponse(OperationResponsePdu {
                            response_code: ResponseCode::OperationNotSupported,
                            transaction_id: op.transaction_id,
                            params: Vec::new(),
                        });
                        stream.write_all(&resp.encode()).await?;
                    }
                },
                _ => {}
            }
        }

        Ok(())
    }
}
