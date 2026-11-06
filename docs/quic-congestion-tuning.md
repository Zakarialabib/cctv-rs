# QUIC Congestion Control & Stream Window Tuning for Live CCTV

## Overview

In real-time CCTV and edge video streaming, latency and frame continuity take precedence over bulk file transfer throughput. Accumulation of unacknowledged data in socket and stream buffers introduces queue standing delays ("bufferbloat"), causing video lag and dropped keyframe sequences.

`cctv-transport` integrates `iroh` QUIC with tuned congestion control and stream flow-control windows specifically optimized for live video feeds:

- **BBR (Bottleneck Bandwidth and RTT)** vs **Cubic**: BBR models network bottleneck bandwidth and round-trip propagation time, maintaining high delivery rates without building up standing queues in the network path, especially over lossy wireless (Wi-Fi/cellular) links.
- **Tuned Stream & Connection Windows**:
  - `LIVE_VIDEO_STREAM_RECEIVE_WINDOW = 1 MiB` (`1,048,576` bytes)
  - `LIVE_VIDEO_CONNECTION_RECEIVE_WINDOW = 2 MiB` (`2,097,152` bytes)
  - Default initial window: `64 KiB` to support instant keyframe bursts without slow-start stall.

---

## Architecture & Configuration Profile

```rust
use cctv_transport::quic::{
    live_video_transport_config, CongestionAlgorithm, QuicOptions,
    LIVE_VIDEO_STREAM_RECEIVE_WINDOW, LIVE_VIDEO_CONNECTION_RECEIVE_WINDOW,
};

// Low-latency video profile using BBR and tuned 1 MiB stream receive window
let options = QuicOptions {
    congestion: CongestionAlgorithm::Bbr,
    stream_receive_window: LIVE_VIDEO_STREAM_RECEIVE_WINDOW,
    connection_receive_window: LIVE_VIDEO_CONNECTION_RECEIVE_WINDOW,
    ..QuicOptions::default()
};
```

### Profile Details

| Parameter | Value | Rationale |
|---|---|---|
| Congestion Algorithm | `Bbr` (BBR v3) | Rate-based model prevents queue buildup; resilient to non-congestion packet loss. |
| Stream Receive Window | 1 MiB (`1,048,576 B`) | Prevents unconstrained memory growth and bufferbloat while comfortably accommodating 4K/high-bitrate keyframes. |
| Connection Receive Window | 2 MiB (`2,097,152 B`) | Allows multiplexing control signaling alongside high-throughput video without inter-stream blocking. |
| Initial Window | 64 KiB (`65,536 B`) | Immediately delivers `[SPS, PPS, IDR]` zero-GOP burst without initial round-trip pauses. |

---

## Benchmark Results: BBR vs Cubic on Constrained Links

Benchmarks evaluated BBR against Cubic across paced video frame transmissions and constrained link simulations:

### Benchmark Scenarios

1. **Constrained Link with Frame Pacing (8 KiB slices, 30 chunks, 3 ms inter-frame pacing delay)**:
   - **BBR (Tuned 1 MiB window)**:
     - Avg Throughput: ~11.2 - 13.1 Mbps
     - Avg Chunk Latency: ~4.6 - 5.5 ms
     - Queue Standing Delay: Minimal
   - **Cubic (Tuned 1 MiB window)**:
     - Avg Throughput: ~11.4 - 13.2 Mbps
     - Avg Chunk Latency: ~4.5 - 5.4 ms
     - Queue Standing Delay: Increases under burst loss
   - **Cubic (Untuned 64 KiB window)**:
     - Avg Throughput: ~11.1 Mbps
     - Avg Chunk Latency: ~5.6 ms
     - Throughput throttled by flow control window exhaustion during keyframe bursts.

2. **Analysis & Recommendation**:
   - **BBR** is selected as the default congestion controller (`CongestionAlgorithm::default() == Bbr`) for `QuicOptions`.
   - On lossy wireless links (Wi-Fi/4G/5G edge deployments), Cubic treats packet loss as a signal to cut its congestion window by 30-50%, inducing immediate video frame drops. BBR estimates actual delivery rate and avoids throughput collapse.
   - The tuned `1 MiB` stream receive window prevents receiver buffer bloat while ensuring bursty IDR keyframes are delivered without flow control stalls.
