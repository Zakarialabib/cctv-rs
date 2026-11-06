//! Integration tests for QUIC transport via iroh (feature `quic`).
//!
//! Verifies:
//! - Local endpoint binding and peer-to-peer connection establishment.
//! - Control and video bidirectional stream multiplexing on a single connection.
//! - Conversion of `QuicStream` into the crate's `net::Stream` socket abstraction.
//! - Connection migration / network change triggers.

#![cfg(feature = "quic")]

use std::time::Duration;

use cctv_transport::net::Stream;
use cctv_transport::quic::{ChannelKind, QuicEndpoint, QuicOptions};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

#[tokio::test]
async fn quic_endpoint_binds_and_connects() {
    let options = QuicOptions {
        alpns: vec![b"cctv/test/1".to_vec()],
        ..QuicOptions::default()
    };

    let server_ep = QuicEndpoint::bind(&options)
        .await
        .expect("server endpoint binds");
    let client_ep = QuicEndpoint::bind(&options)
        .await
        .expect("client endpoint binds");

    let server_addr = server_ep.addr();
    let server_task = tokio::spawn(async move {
        let conn = server_ep
            .accept()
            .await
            .expect("server accepts")
            .expect("connection exists");
        let (mut send, mut recv) = conn.open_bi().await.expect("open_bi").into_split();
        send.write_all(b"ping").await.expect("write ping");
        send.finish().expect("finish");
        let mut buf = vec![0u8; 4];
        recv.read_exact(&mut buf).await.expect("read pong");
        assert_eq!(&buf, b"pong");
    });

    let client_conn = client_ep
        .connect(server_addr, b"cctv/test/1", Duration::from_secs(5))
        .await
        .expect("client connects");

    let mut stream = client_conn.accept_bi().await.expect("accept_bi");
    let mut buf = vec![0u8; 4];
    stream.read_exact(&mut buf).await.expect("read ping");
    assert_eq!(&buf, b"ping");
    stream.write_all(b"pong").await.expect("write pong");
    stream.flush().await.expect("flush pong");

    server_task.await.expect("server task completes");
}

#[tokio::test]
async fn quic_multiplexes_control_and_video_channels_on_one_connection() {
    let options = QuicOptions {
        alpns: vec![b"cctv/mux/1".to_vec()],
        ..QuicOptions::default()
    };

    let server_ep = QuicEndpoint::bind(&options)
        .await
        .expect("server endpoint binds");
    let client_ep = QuicEndpoint::bind(&options)
        .await
        .expect("client endpoint binds");

    let server_addr = server_ep.addr();

    // Server accepts multiplexed channels and echoes them appropriately.
    let server_task = tokio::spawn(async move {
        let conn = server_ep
            .accept()
            .await
            .expect("accept connection")
            .expect("some conn");

        // We handle two channels: one Control, one Video.
        for _ in 0..2 {
            let (kind, mut stream) = conn.accept_channel().await.expect("accept channel");
            match kind {
                ChannelKind::Control => {
                    let mut cmd = [0u8; 4];
                    stream
                        .read_exact(&mut cmd)
                        .await
                        .expect("read control command");
                    assert_eq!(&cmd, b"PTZ1");
                    stream.write_all(b"ACK1").await.expect("ack control");
                    stream.flush().await.expect("flush");
                }
                ChannelKind::Video => {
                    let mut frame = vec![0u8; 1024];
                    stream
                        .read_exact(&mut frame)
                        .await
                        .expect("read video payload");
                    assert_eq!(frame[0], 0xFF);
                    stream.write_all(b"FRAMEOK").await.expect("ack video");
                    stream.flush().await.expect("flush");
                }
                ChannelKind::Custom(tag) => panic!("unexpected channel tag: {tag}"),
            }
            // Keep stream open until client finishes reading.
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        // Keep server endpoint and connection alive until client finishes.
        tokio::time::sleep(Duration::from_millis(500)).await;
    });

    let client_conn = client_ep
        .connect(server_addr, b"cctv/mux/1", Duration::from_secs(5))
        .await
        .expect("client connects");

    // 1. Send Control stream
    let mut ctrl_stream = client_conn
        .open_channel(ChannelKind::Control)
        .await
        .expect("open control channel");
    ctrl_stream
        .write_all(b"PTZ1")
        .await
        .expect("send control command");
    ctrl_stream.flush().await.expect("flush control command");

    // Read control response
    let mut ctrl_ack = [0u8; 4];
    ctrl_stream
        .read_exact(&mut ctrl_ack)
        .await
        .expect("read ctrl ack");
    assert_eq!(&ctrl_ack, b"ACK1");

    // 2. Concurrently send Video stream on the *same* connection
    let mut video_stream = client_conn
        .open_channel(ChannelKind::Video)
        .await
        .expect("open video channel");
    let frame_payload = vec![0xFFu8; 1024];
    video_stream
        .write_all(&frame_payload)
        .await
        .expect("send video frame");
    video_stream.flush().await.expect("flush video frame");

    let mut video_ack = [0u8; 7];
    video_stream
        .read_exact(&mut video_ack)
        .await
        .expect("read video ack");
    assert_eq!(&video_ack, b"FRAMEOK");

    server_task
        .await
        .expect("server task completed successfully");
}

#[tokio::test]
async fn quic_stream_fits_behind_net_stream_abstraction() {
    let options = QuicOptions {
        alpns: vec![b"cctv/stream/1".to_vec()],
        ..QuicOptions::default()
    };

    let server_ep = QuicEndpoint::bind(&options).await.expect("bind server");
    let client_ep = QuicEndpoint::bind(&options).await.expect("bind client");
    let server_addr = server_ep.addr();

    let server_task = tokio::spawn(async move {
        let conn = server_ep.accept().await.unwrap().unwrap();
        let (mut send, recv) = conn.open_bi().await.unwrap().into_split();
        send.write_all(b"hello").await.unwrap();
        send.flush().await.unwrap();

        let mut stream = Stream::from(cctv_transport::quic::QuicStream::new(send, recv));
        assert_eq!(stream.to_string(), "quic");

        let mut buf = [0u8; 5];
        stream.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"world");
    });

    let client_conn = client_ep
        .connect(server_addr, b"cctv/stream/1", Duration::from_secs(5))
        .await
        .unwrap();

    let q_stream = client_conn.accept_bi().await.unwrap();
    let mut stream = Stream::from(q_stream);

    assert_eq!(stream.to_string(), "quic");

    let mut resp = [0u8; 5];
    stream.read_exact(&mut resp).await.unwrap();
    assert_eq!(&resp, b"hello");

    stream.write_all(b"world").await.unwrap();
    stream.flush().await.unwrap();

    server_task.await.unwrap();
}

#[tokio::test]
async fn quic_network_change_invocations_succeed() {
    let options = QuicOptions::default();
    let endpoint = QuicEndpoint::bind(&options).await.expect("bind endpoint");

    // Simulates roaming from Wi-Fi to cellular (mobile / desktop interface switch).
    endpoint.network_change().await;
}

#[tokio::test]
async fn quic_pacing_aware_keyframe_request_round_trip() {
    use cctv_transport::pacing::{
        KeyframeRequestReason, KeyframeRequester, KeyframeRequesterConfig, PacingSender, PacketType,
    };

    let options = QuicOptions {
        alpns: vec![b"cctv/keyframe/1".to_vec()],
        ..QuicOptions::default()
    };

    let server_ep = QuicEndpoint::bind(&options).await.expect("server binds");
    let client_ep = QuicEndpoint::bind(&options).await.expect("client binds");
    let server_addr = server_ep.addr();

    // Server represents the camera receiving keyframe requests on the control channel
    let server_task = tokio::spawn(async move {
        let conn = server_ep.accept().await.unwrap().unwrap();
        let (kind, mut stream) = conn.accept_channel().await.unwrap();
        assert_eq!(kind, ChannelKind::Control);

        let mut magic = [0u8; 4];
        stream.read_exact(&mut magic).await.unwrap();
        assert_eq!(&magic, b"KEYF");

        let mut seq_bytes = [0u8; 8];
        stream.read_exact(&mut seq_bytes).await.unwrap();
        let seq = u64::from_be_bytes(seq_bytes);
        assert_eq!(seq, 1);

        let mut reason_byte = [0u8; 1];
        stream.read_exact(&mut reason_byte).await.unwrap();
        assert_eq!(reason_byte[0], 2); // CongestionDrop tag

        // Camera acknowledges keyframe request and confirms IDR generation
        stream.write_all(b"IDR_OK").await.unwrap();
        stream.flush().await.unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;
    });

    let client_conn = client_ep
        .connect(server_addr, b"cctv/keyframe/1", Duration::from_secs(5))
        .await
        .unwrap();

    let mut ctrl_stream = client_conn
        .open_channel(ChannelKind::Control)
        .await
        .unwrap();

    // Client has a PacingSender that encounters congestion
    let mut sender = PacingSender::new(1);
    let mut requester = KeyframeRequester::new(KeyframeRequesterConfig::default());

    let now = 1_000;
    sender.enqueue_video(PacketType::DeltaFrame, vec![0; 500], true, now);
    // Overflow queue to trigger drop
    sender.enqueue_video(PacketType::DeltaFrame, vec![0; 500], true, now + 33);

    let maybe_req = requester.poll_request(&mut sender, now + 33);
    assert!(maybe_req.is_some());
    let req = maybe_req.unwrap();
    assert_eq!(req.reason, KeyframeRequestReason::CongestionDrop);

    // Send control message over the QUIC control stream
    ctrl_stream.write_all(&req.payload).await.unwrap();
    ctrl_stream.flush().await.unwrap();

    let mut ack = [0u8; 6];
    ctrl_stream.read_exact(&mut ack).await.unwrap();
    assert_eq!(&ack, b"IDR_OK");

    server_task.await.unwrap();
}
