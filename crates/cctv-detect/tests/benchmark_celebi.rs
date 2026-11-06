//! Benchmarks comparing sequential vs Rayon-parallelized Wsmeans clustering in Celebi Quantizer.

use cctv_detect::quantize::{Argb, QuantizerCelebi};
use std::time::Instant;

#[test]
fn benchmark_wsmeans_speedup() {
    // Generate a high-resolution 1920x1080 test frame with rich color gradients
    let width = 1920;
    let height = 1080;
    let mut pixels = Vec::with_capacity((width * height) as usize);

    for y in 0..height {
        for x in 0..width {
            let r = ((x * 255) / width) as u8;
            let g = ((y * 255) / height) as u8;
            let b = (((x ^ y) * 255) / width.max(height)) as u8;
            pixels.push(Argb::from_rgb(r, g, b).to_u32());
        }
    }

    let celebi = QuantizerCelebi::new(32);

    // Warm-up
    let _ = celebi.quantize_sequential(&pixels[..10000], 100, 100);
    let _ = celebi.quantize_parallel(&pixels[..10000], 100, 100);

    // Benchmark Sequential
    let iters = 10;
    let t0 = Instant::now();
    for _ in 0..iters {
        let _ = celebi.quantize_sequential(&pixels, width, height);
    }
    let seq_duration = t0.elapsed();

    // Benchmark Parallel (Rayon)
    let t1 = Instant::now();
    for _ in 0..iters {
        let _ = celebi.quantize_parallel(&pixels, width, height);
    }
    let par_duration = t1.elapsed();

    let seq_ms = seq_duration.as_secs_f64() * 1000.0 / iters as f64;
    let par_ms = par_duration.as_secs_f64() * 1000.0 / iters as f64;
    let speedup = seq_ms / par_ms;

    println!(
        "\n=== Celebi Quantizer Speedup Benchmark (1920x1080 -> 112x112 downsampled, k=32) ==="
    );
    println!("Sequential Wsmeans: {:.2} ms / iter", seq_ms);
    println!("Parallel Wsmeans:   {:.2} ms / iter", par_ms);
    println!("Measured Speedup:   {:.2}x\n", speedup);

    // Speedup on multi-core is expected to reduce palette generation latency significantly
    assert!(
        par_ms < seq_ms * 1.5,
        "Parallel path should not be excessively slower than sequential even with thread overhead"
    );
}
