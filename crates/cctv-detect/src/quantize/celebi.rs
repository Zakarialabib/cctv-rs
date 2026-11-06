//! Celebi Quantizer.
//!
//! An image quantizer combining:
//! 1. Downsampling large images to at most 112x112 pixels.
//! 2. Wu quantizer to find an initial palette.
//! 3. Wsmeans (Weighted Square Means) clustering in CIE Lab space, accelerated with
//!    triangle inequality pruning and parallelized via Rayon.

use std::collections::HashMap;

use super::color::{Argb, Lab};
use super::wsmeans::{cluster_wsmeans, cluster_wsmeans_sequential, points_from_histogram};
use super::wu::QuantizerWu;

#[cfg(feature = "parallel")]
use super::wsmeans::cluster_wsmeans_parallel;

/// Target maximum edge dimension for downsampling (112x112).
pub const MAX_IMAGE_DIMENSION: u32 = 112;

/// Quantizer Celebi configuration and runner.
pub struct QuantizerCelebi {
    max_colors: usize,
}

impl QuantizerCelebi {
    /// Create a new Celebi quantizer producing up to `max_colors` dominant colors.
    pub fn new(max_colors: usize) -> Self {
        Self { max_colors }
    }

    /// Downsample raw RGB/ARGB pixel buffer to <= 112x112 while preserving aspect ratio.
    ///
    /// Input pixels are expected as 32-bit `0xAARRGGBB` or `0x00RRGGBB`.
    pub fn downsample(pixels: &[u32], width: u32, height: u32) -> (Vec<u32>, u32, u32) {
        if width == 0 || height == 0 || pixels.len() < (width * height) as usize {
            return (Vec::new(), 0, 0);
        }

        if width <= MAX_IMAGE_DIMENSION && height <= MAX_IMAGE_DIMENSION {
            return (pixels[..(width * height) as usize].to_vec(), width, height);
        }

        let scale = (MAX_IMAGE_DIMENSION as f64) / (width.max(height) as f64);
        let target_w = ((width as f64 * scale).round() as u32).max(1);
        let target_h = ((height as f64 * scale).round() as u32).max(1);

        let mut downsampled = Vec::with_capacity((target_w * target_h) as usize);

        for y_out in 0..target_h {
            let y_in = ((y_out as f64 / target_h as f64) * height as f64) as u32;
            let row_offset = (y_in.min(height - 1) * width) as usize;

            for x_out in 0..target_w {
                let x_in = ((x_out as f64 / target_w as f64) * width as f64) as u32;
                let col = x_in.min(width - 1) as usize;
                downsampled.push(pixels[row_offset + col]);
            }
        }

        (downsampled, target_w, target_h)
    }

    /// Count pixel occurrences to construct a color frequency histogram.
    pub fn histogram(pixels: &[u32]) -> HashMap<u32, u32> {
        let mut counts = HashMap::new();
        for &px in pixels {
            // Normalize alpha to fully opaque if 0
            let normalized = if (px >> 24) == 0 {
                px | 0xFF00_0000
            } else {
                px
            };
            *counts.entry(normalized).or_insert(0) += 1;
        }
        counts
    }

    /// Quantize an image from raw pixels (width, height) using Celebi.
    /// Returns dominant colors sorted by prominence / cluster weight.
    pub fn quantize(&self, pixels: &[u32], width: u32, height: u32) -> Vec<u32> {
        let (downsampled, _w, _h) = Self::downsample(pixels, width, height);
        let hist = Self::histogram(&downsampled);
        self.quantize_histogram(&hist)
    }

    /// Quantize from a color frequency histogram.
    pub fn quantize_histogram(&self, hist: &HashMap<u32, u32>) -> Vec<u32> {
        if hist.is_empty() || self.max_colors == 0 {
            return Vec::new();
        }

        if hist.len() <= self.max_colors {
            return hist.keys().copied().collect();
        }

        // 1. Initial palette using Wu quantizer
        let mut wu = QuantizerWu::new();
        let wu_colors = wu.quantize(hist, self.max_colors);

        let initial_clusters: Vec<Lab> = wu_colors
            .iter()
            .map(|&c| Argb::from_u32(c).to_lab())
            .collect();

        // 2. Cluster using Wsmeans
        let points = points_from_histogram(hist);
        let final_clusters = cluster_wsmeans(&points, initial_clusters);

        // Convert back to ARGB
        final_clusters
            .into_iter()
            .map(|lab| lab.to_argb().to_u32())
            .collect()
    }

    /// Quantize using the sequential Wsmeans path (for comparison / benchmarks).
    pub fn quantize_sequential(&self, pixels: &[u32], width: u32, height: u32) -> Vec<u32> {
        let (downsampled, _w, _h) = Self::downsample(pixels, width, height);
        let hist = Self::histogram(&downsampled);
        if hist.is_empty() || self.max_colors == 0 {
            return Vec::new();
        }
        if hist.len() <= self.max_colors {
            return hist.keys().copied().collect();
        }
        let mut wu = QuantizerWu::new();
        let wu_colors = wu.quantize(&hist, self.max_colors);
        let initial_clusters: Vec<Lab> = wu_colors
            .iter()
            .map(|&c| Argb::from_u32(c).to_lab())
            .collect();
        let points = points_from_histogram(&hist);
        let final_clusters = cluster_wsmeans_sequential(&points, initial_clusters);
        final_clusters
            .into_iter()
            .map(|lab| lab.to_argb().to_u32())
            .collect()
    }

    /// Quantize using the Rayon parallel Wsmeans path explicitly.
    #[cfg(feature = "parallel")]
    pub fn quantize_parallel(&self, pixels: &[u32], width: u32, height: u32) -> Vec<u32> {
        let (downsampled, _w, _h) = Self::downsample(pixels, width, height);
        let hist = Self::histogram(&downsampled);
        if hist.is_empty() || self.max_colors == 0 {
            return Vec::new();
        }
        if hist.len() <= self.max_colors {
            return hist.keys().copied().collect();
        }
        let mut wu = QuantizerWu::new();
        let wu_colors = wu.quantize(&hist, self.max_colors);
        let initial_clusters: Vec<Lab> = wu_colors
            .iter()
            .map(|&c| Argb::from_u32(c).to_lab())
            .collect();
        let points = points_from_histogram(&hist);
        let final_clusters = cluster_wsmeans_parallel(&points, initial_clusters);
        final_clusters
            .into_iter()
            .map(|lab| lab.to_argb().to_u32())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_downsample_bounds() {
        let w = 1920;
        let h = 1080;
        let pixels = vec![0xFF112233; (w * h) as usize];
        let (downsampled, target_w, target_h) = QuantizerCelebi::downsample(&pixels, w, h);

        assert!(target_w <= MAX_IMAGE_DIMENSION);
        assert!(target_h <= MAX_IMAGE_DIMENSION);
        assert_eq!(downsampled.len(), (target_w * target_h) as usize);
        assert_eq!(target_w, 112);
        assert_eq!(target_h, 63); // 1080 * (112/1920) = 63
    }

    #[test]
    fn test_quantize_simple_image() {
        // Red, Green, Blue image
        let mut pixels = Vec::new();
        pixels.extend(std::iter::repeat_n(0xFFFF0000, 5000));
        pixels.extend(std::iter::repeat_n(0xFF00FF00, 5000));
        pixels.extend(std::iter::repeat_n(0xFF0000FF, 5000));

        let celebi = QuantizerCelebi::new(3);
        let palette = celebi.quantize(&pixels, 150, 100);

        assert_eq!(palette.len(), 3);
        // Ensure all primary hues are represented in the palette
        let has_red = palette.iter().any(|&c| {
            let argb = Argb::from_u32(c);
            argb.r > 200 && argb.g < 50 && argb.b < 50
        });
        let has_green = palette.iter().any(|&c| {
            let argb = Argb::from_u32(c);
            argb.g > 200 && argb.r < 50 && argb.b < 50
        });
        let has_blue = palette.iter().any(|&c| {
            let argb = Argb::from_u32(c);
            argb.b > 200 && argb.r < 50 && argb.g < 50
        });

        assert!(has_red, "Palette should contain red");
        assert!(has_green, "Palette should contain green");
        assert!(has_blue, "Palette should contain blue");
    }

    #[test]
    #[cfg(feature = "parallel")]
    fn test_sequential_and_parallel_consistency() {
        // Generate diverse image
        let mut pixels = Vec::with_capacity(300 * 300);
        for y in 0..300 {
            for x in 0..300 {
                let r = ((x * 255) / 300) as u32;
                let g = ((y * 255) / 300) as u32;
                let b = (((x + y) * 255) / 600) as u32;
                pixels.push(0xFF00_0000 | (r << 16) | (g << 8) | b);
            }
        }

        let celebi = QuantizerCelebi::new(16);
        let seq_palette = celebi.quantize_sequential(&pixels, 300, 300);
        let par_palette = celebi.quantize_parallel(&pixels, 300, 300);

        assert_eq!(seq_palette.len(), par_palette.len());
        // Verify both palettes produce very close cluster centers
        for &seq_c in &seq_palette {
            let seq_lab = Argb::from_u32(seq_c).to_lab();
            let min_dist = par_palette
                .iter()
                .map(|&par_c| seq_lab.distance(&Argb::from_u32(par_c).to_lab()))
                .fold(f64::INFINITY, f64::min);
            assert!(
                min_dist < 5.0,
                "Cluster center discrepancy between seq and par: {min_dist}"
            );
        }
    }
}
