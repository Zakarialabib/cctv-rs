//! Benchmarks comparing BBR against Cubic congestion control on constrained links for live video.

#![cfg(feature = "quic")]

use std::time::{Duration, Instant};

use cctv_transport::quic::{
    ChannelKind, CongestionAlgorithm, QuicEndpoint, QuicOptions,
    LIVE_VIDEO_CONNECTION_RECEIVE_WINDOW, LIVE_VIDEO_STREAM_RECEIVE_WINDOW,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[allow(dead_code)]
struct BenchmarkResult {
    algorithm: CongestionAlgorithm,
    total_bytes: usize,
    duration: Duration,
    throughput_kbps: f64,
    avg_chunk_latency_ms: f64,
}

async fn run_benchmark(
    algorithm: CongestionAlgorithm,
    chunk_size: usize,
    num_chunks: usize,
    chunk_delay: Duration,
    stream_window: u32,
) -> BenchmarkResult {
    let alpn = format!("cctv/bench/{:?}/{}", algorithm, stream_window).into_bytes();
    let options = QuicOptions {
        alpns: vec![alpn.clone()],
        congestion: algorithm,
        stream_receive_window: stream_window,
        connection_receive_window: LIVE_VIDEO_CONNECTION_RECEIVE_WINDOW,
        ..QuicOptions::default()
    };

    let server_ep = QuicEndpoint::bind(&options)
        .await
        .expect("server endpoint binds");
    let client_ep = QuicEndpoint::bind(&options)
        .await
        .expect("client endpoint binds");

    let server_addr = server_ep.addr();
    let total_expected_bytes = chunk_size * num_chunks;

    let server_task = tokio::spawn(async move {
        let conn = server_ep.accept().await.unwrap().unwrap();
        let (_kind, mut stream) = conn.accept_channel().await.unwrap();

        let mut received = 0;
        let mut buf = vec![0u8; chunk_size];
        let mut chunk_latencies = Vec::new();

        while received < total_expected_bytes {
            let start = Instant::now();
            stream.read_exact(&mut buf).await.unwrap();
            chunk_latencies.push(start.elapsed().as_secs_f64() * 1000.0);
            received += buf.len();

            // Send feedback ACK
            stream.write_all(&[0x06]).await.unwrap();
            stream.flush().await.unwrap();
        }

        let avg_latency = if chunk_latencies.is_empty() {
            0.0
        } else {
            chunk_latencies.iter().sum::<f64>() / chunk_latencies.len() as f64
        };

        (received, avg_latency, server_ep, conn)
    });

    let client_conn = client_ep
        .connect(server_addr, &alpn, Duration::from_secs(5))
        .await
        .expect("client connects");

    let mut stream = client_conn
        .open_channel(ChannelKind::Video)
        .await
        .expect("open video channel");

    let bench_start = Instant::now();
    let chunk = vec![0xAAu8; chunk_size];

    for _ in 0..num_chunks {
        stream.write_all(&chunk).await.unwrap();
        stream.flush().await.unwrap();

        let mut ack = [0u8; 1];
        stream.read_exact(&mut ack).await.unwrap();

        if !chunk_delay.is_zero() {
            tokio::time::sleep(chunk_delay).await;
        }
    }

    let duration = bench_start.elapsed();
    let (received, avg_chunk_latency_ms, _ep, _conn) = server_task.await.unwrap();

    let throughput_kbps = (received as f64 * 8.0) / (duration.as_secs_f64() * 1000.0);

    BenchmarkResult {
        algorithm,
        total_bytes: received,
        duration,
        throughput_kbps,
        avg_chunk_latency_ms,
    }
}

#[tokio::test]
async fn benchmark_bbr_vs_cubic_live_video_constrained_link() {
    let chunk_size = 8 * 1024; // 8 KiB video packet slice
    let num_chunks = 30; // 240 KiB total per run
    let throttle_delay = Duration::from_millis(3); // Simulates frame pacing delay on constrained link

    // Run BBR with tuned stream receive window (1 MiB)
    let bbr_tuned = run_benchmark(
        CongestionAlgorithm::Bbr,
        chunk_size,
        num_chunks,
        throttle_delay,
        LIVE_VIDEO_STREAM_RECEIVE_WINDOW,
    )
    .await;

    // Run Cubic with tuned stream receive window (1 MiB)
    let cubic_tuned = run_benchmark(
        CongestionAlgorithm::Cubic,
        chunk_size,
        num_chunks,
        throttle_delay,
        LIVE_VIDEO_STREAM_RECEIVE_WINDOW,
    )
    .await;

    // Run Cubic with small/untuned stream receive window (64 KiB)
    let cubic_small_window = run_benchmark(
        CongestionAlgorithm::Cubic,
        chunk_size,
        num_chunks,
        throttle_delay,
        64 * 1024,
    )
    .await;

    println!("\n================ QUIC Live Video Benchmark Results ================");
    println!(
        "BBR (Tuned 1MB window)       -> Duration: {:?}, Throughput: {:.2} kbps, Avg Chunk Latency: {:.3} ms",
        bbr_tuned.duration, bbr_tuned.throughput_kbps, bbr_tuned.avg_chunk_latency_ms
    );
    println!(
        "Cubic (Tuned 1MB window)     -> Duration: {:?}, Throughput: {:.2} kbps, Avg Chunk Latency: {:.3} ms",
        cubic_tuned.duration, cubic_tuned.throughput_kbps, cubic_tuned.avg_chunk_latency_ms
    );
    println!(
        "Cubic (Untuned 64KB window)  -> Duration: {:?}, Throughput: {:.2} kbps, Avg Chunk Latency: {:.3} ms",
        cubic_small_window.duration, cubic_small_window.throughput_kbps, cubic_small_window.avg_chunk_latency_ms
    );
    println!("===================================================================\n");

    assert_eq!(bbr_tuned.total_bytes, chunk_size * num_chunks);
    assert_eq!(cubic_tuned.total_bytes, chunk_size * num_chunks);
    assert_eq!(cubic_small_window.total_bytes, chunk_size * num_chunks);

    // Verify low-latency video profile performance
    assert!(bbr_tuned.throughput_kbps > 0.0);
    assert!(cubic_tuned.throughput_kbps > 0.0);
}
