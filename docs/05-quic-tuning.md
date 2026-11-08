# 05 — QUIC Congestion Control & Stream-Window Tuning

> The transport profile `cctv-transport` applies to live CCTV feeds: latency and
> frame continuity take precedence over bulk throughput.

**Read this when** you are tuning a QUIC endpoint, choosing a congestion
controller, or wondering why the receive windows are set where they are.

---

## 1. Why latency, not throughput

In real-time CCTV and edge video streaming, unacknowledged data accumulating in
socket and stream buffers introduces queue standing delay ("bufferbloat"),
causing video lag and dropped keyframe sequences. `cctv-transport` therefore
integrates `iroh` QUIC with congestion control and stream flow-control windows
tuned specifically for live video feeds:

- **BBR (bottleneck bandwidth and RTT) vs. Cubic.** BBR models the network
  bottleneck bandwidth and round-trip propagation time and holds a high delivery
  rate without building standing queues in the path — especially over lossy
  wireless (Wi-Fi / cellular) links.
- **Tuned stream and connection windows**, sized so bursty IDR keyframes are
  delivered without flow-control stalls while receiver memory stays bounded.

---

## 2. The live-video profile

```rust
use cctv_transport::quic::{
    live_video_transport_config, CongestionAlgorithm, QuicOptions,
    LIVE_VIDEO_STREAM_RECEIVE_WINDOW, LIVE_VIDEO_CONNECTION_RECEIVE_WINDOW,
};

// Low-latency video profile: BBR with a tuned 1 MiB stream receive window.
let options = QuicOptions {
    congestion: CongestionAlgorithm::Bbr,
    stream_receive_window: LIVE_VIDEO_STREAM_RECEIVE_WINDOW,
    connection_receive_window: LIVE_VIDEO_CONNECTION_RECEIVE_WINDOW,
    ..QuicOptions::default()
};
```

| Parameter | Value | Rationale |
| --- | --- | --- |
| Congestion algorithm | `Bbr` (BBR v3) | Rate-based model prevents queue buildup; resilient to non-congestion packet loss. |
| Stream receive window | 1 MiB (`1_048_576 B`) | Prevents unbounded memory growth and bufferbloat while comfortably accommodating 4K / high-bitrate keyframes. |
| Connection receive window | 2 MiB (`2_097_152 B`) | Multiplexes control signalling alongside high-throughput video without inter-stream blocking. |
| Initial window | 64 KiB (`65_536 B`) | Delivers the `[SPS, PPS, IDR]` zero-GOP burst immediately, without an initial round-trip pause. |

---

## 3. Benchmark: BBR vs. Cubic on constrained links

Benchmarks compared BBR against Cubic across paced video-frame transmissions and
constrained-link simulations.

**Scenario: constrained link with frame pacing** (8 KiB slices, 30 chunks, 3 ms
inter-frame pacing delay):

| Profile | Avg. throughput | Avg. chunk latency | Queue standing delay |
| --- | --- | --- | --- |
| BBR, tuned 1 MiB window | ~11.2–13.1 Mbps | ~4.6–5.5 ms | Minimal |
| Cubic, tuned 1 MiB window | ~11.4–13.2 Mbps | ~4.5–5.4 ms | Increases under burst loss |
| Cubic, untuned 64 KiB window | ~11.1 Mbps | ~5.6 ms | Flow-control window exhaustion during keyframe bursts |

---

## 4. Recommendation

- **BBR is the default** (`CongestionAlgorithm::default() == Bbr`).
- On lossy wireless links (Wi-Fi / 4G / 5G edge deployments), Cubic treats packet
  loss as a signal to cut its congestion window by 30–50%, inducing immediate
  video frame drops. BBR estimates the actual delivery rate and avoids the
  throughput collapse.
- The tuned **1 MiB stream receive window** prevents receiver buffer bloat while
  ensuring bursty IDR keyframes are delivered without flow-control stalls.

---

## See also

- [01 — Architecture](01-architecture.md) §4 — QUIC in the protocol matrix.

---

## License & Security

- **License:** Apache-2.0 ([LICENSE](../LICENSE)).
- **Security:** see [SECURITY.md](../SECURITY.md) for the vulnerability disclosure
  process.
