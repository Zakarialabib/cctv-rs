//! Round-trip test for PTP protocol client against mock responder.

use cctv_protocol_ptp::{MockPtpResponder, PtpClient};

#[tokio::test]
async fn test_ptp_roundtrip_with_mock_responder() {
    let mock = MockPtpResponder::bind().await.unwrap();
    let addr = mock.local_addr();

    let mock_task = tokio::spawn(async move {
        mock.run().await.unwrap();
    });

    let mut client = PtpClient::connect(&addr).await.unwrap();

    let ack = client.init_session([1u8; 16], "TestHost").await.unwrap();
    assert_eq!(ack.responder_name, "Mock-PTP");

    client.open_session(1).await.unwrap();

    let dev_info = client.get_device_info().await.unwrap();
    assert_eq!(dev_info.manufacturer, "CCTV Mock");
    assert_eq!(dev_info.model, "Pro-Cam 9000");

    let storage_ids = client.get_storage_ids().await.unwrap();
    assert_eq!(storage_ids, vec![0x0001_0001]);

    client.close_session().await.unwrap();

    mock_task.abort();
}
