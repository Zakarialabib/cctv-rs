//! Weighted Square Means (Wsmeans) Clustering.
//!
//! An efficient k-means variant operating in CIE L*a*b* color space, accelerated with
//! triangle inequality pruning to eliminate redundant point-to-center distance computations.
//!
//! This module provides both sequential and Rayon-parallelized implementations of the point
//! reassignment loop.

use std::collections::HashMap;

use super::color::{Argb, Lab};

#[cfg(feature = "parallel")]
use rayon::prelude::*;

const MAX_ITERATIONS: usize = 10;
const MIN_MOVEMENT_DISTANCE_SQ: f64 = 3.0;

/// A weighted color point extracted from the downsampled image.
#[derive(Debug, Clone, Copy)]
pub struct WeightedPoint {
    /// CIE L*a*b* coordinates.
    pub lab: Lab,
    /// Frequency/weight of this point (pixel count).
    pub weight: u32,
    /// Original ARGB representation.
    pub argb: u32,
}

/// Computes pairwise distances between cluster centers.
fn compute_cluster_distances(clusters: &[Lab]) -> Vec<Vec<f64>> {
    let k = clusters.len();
    let mut dists = vec![vec![0.0; k]; k];
    for i in 0..k {
        for j in (i + 1)..k {
            let d = clusters[i].distance(&clusters[j]);
            dists[i][j] = d;
            dists[j][i] = d;
        }
    }
    dists
}

/// Runs the Wsmeans clustering loop sequentially.
pub fn cluster_wsmeans_sequential(points: &[WeightedPoint], mut clusters: Vec<Lab>) -> Vec<Lab> {
    let k = clusters.len();
    if k == 0 || points.is_empty() {
        return clusters;
    }

    let n = points.len();
    let mut cluster_indices = vec![0usize; n];
    // Cache of point distance to its assigned cluster center
    let mut point_to_cluster_dist = vec![f64::INFINITY; n];

    // Initial assignment
    for (i, p) in points.iter().enumerate() {
        let mut min_d = f64::INFINITY;
        let mut best_c = 0;
        for (c_idx, c) in clusters.iter().enumerate() {
            let d = p.lab.distance(c);
            if d < min_d {
                min_d = d;
                best_c = c_idx;
            }
        }
        cluster_indices[i] = best_c;
        point_to_cluster_dist[i] = min_d;
    }

    for _iteration in 0..MAX_ITERATIONS {
        let cluster_dists = compute_cluster_distances(&clusters);

        // Distance each cluster center moves in this iteration
        let mut center_moved = false;

        // Reassignment with triangle inequality pruning
        for (i, p) in points.iter().enumerate() {
            let current_c = cluster_indices[i];
            let mut current_d = p.lab.distance(&clusters[current_c]);
            point_to_cluster_dist[i] = current_d;

            for other_c in 0..k {
                if other_c == current_c {
                    continue;
                }
                // Triangle inequality check:
                // If dist(current_c, other_c) >= 2 * dist(point, current_c),
                // then other_c cannot be closer to point than current_c.
                if cluster_dists[current_c][other_c] >= 2.0 * current_d {
                    continue;
                }

                let d = p.lab.distance(&clusters[other_c]);
                if d < current_d {
                    current_d = d;
                    cluster_indices[i] = other_c;
                    point_to_cluster_dist[i] = d;
                }
            }
        }

        // Recompute cluster centroids
        let mut sum_l = vec![0.0; k];
        let mut sum_a = vec![0.0; k];
        let mut sum_b = vec![0.0; k];
        let mut sum_w = vec![0.0; k];

        for (i, p) in points.iter().enumerate() {
            let c = cluster_indices[i];
            let w = p.weight as f64;
            sum_l[c] += p.lab.l * w;
            sum_a[c] += p.lab.a * w;
            sum_b[c] += p.lab.b * w;
            sum_w[c] += w;
        }

        for (c, cluster) in clusters.iter_mut().enumerate().take(k) {
            if sum_w[c] > 0.0 {
                let new_center = Lab::new(
                    sum_l[c] / sum_w[c],
                    sum_a[c] / sum_w[c],
                    sum_b[c] / sum_w[c],
                );
                if cluster.distance_squared(&new_center) > MIN_MOVEMENT_DISTANCE_SQ {
                    center_moved = true;
                }
                *cluster = new_center;
            }
        }

        if !center_moved {
            break;
        }
    }

    clusters
}

/// Runs the Wsmeans clustering loop parallelized across points using Rayon.
#[cfg(feature = "parallel")]
pub fn cluster_wsmeans_parallel(points: &[WeightedPoint], mut clusters: Vec<Lab>) -> Vec<Lab> {
    let k = clusters.len();
    if k == 0 || points.is_empty() {
        return clusters;
    }

    let n = points.len();
    let mut cluster_indices = vec![0usize; n];
    let mut point_to_cluster_dist = vec![f64::INFINITY; n];

    // Initial assignment in parallel
    cluster_indices
        .par_iter_mut()
        .zip(point_to_cluster_dist.par_iter_mut())
        .zip(points.par_iter())
        .for_each(|((c_out, d_out), p)| {
            let mut min_d = f64::INFINITY;
            let mut best_c = 0;
            for (c_idx, c) in clusters.iter().enumerate() {
                let d = p.lab.distance(c);
                if d < min_d {
                    min_d = d;
                    best_c = c_idx;
                }
            }
            *c_out = best_c;
            *d_out = min_d;
        });

    for _iteration in 0..MAX_ITERATIONS {
        let cluster_dists = compute_cluster_distances(&clusters);

        // Parallel reassignment with triangle inequality pruning
        let clusters_ref = &clusters;
        let cluster_dists_ref = &cluster_dists;

        cluster_indices
            .par_iter_mut()
            .zip(point_to_cluster_dist.par_iter_mut())
            .zip(points.par_iter())
            .for_each(|((c_idx_ref, dist_ref), p)| {
                let current_c = *c_idx_ref;
                let mut current_d = p.lab.distance(&clusters_ref[current_c]);
                *dist_ref = current_d;

                for other_c in 0..k {
                    if other_c == current_c {
                        continue;
                    }
                    if cluster_dists_ref[current_c][other_c] >= 2.0 * current_d {
                        continue;
                    }

                    let d = p.lab.distance(&clusters_ref[other_c]);
                    if d < current_d {
                        current_d = d;
                        *c_idx_ref = other_c;
                        *dist_ref = d;
                    }
                }
            });

        // Parallel reduction of centroid sums
        struct CentroidAccumulator {
            sum_l: Vec<f64>,
            sum_a: Vec<f64>,
            sum_b: Vec<f64>,
            sum_w: Vec<f64>,
        }

        impl CentroidAccumulator {
            fn new(k: usize) -> Self {
                Self {
                    sum_l: vec![0.0; k],
                    sum_a: vec![0.0; k],
                    sum_b: vec![0.0; k],
                    sum_w: vec![0.0; k],
                }
            }

            fn add(&mut self, c: usize, p: &WeightedPoint) {
                let w = p.weight as f64;
                self.sum_l[c] += p.lab.l * w;
                self.sum_a[c] += p.lab.a * w;
                self.sum_b[c] += p.lab.b * w;
                self.sum_w[c] += w;
            }

            fn combine(mut self, other: Self) -> Self {
                for i in 0..self.sum_l.len() {
                    self.sum_l[i] += other.sum_l[i];
                    self.sum_a[i] += other.sum_a[i];
                    self.sum_b[i] += other.sum_b[i];
                    self.sum_w[i] += other.sum_w[i];
                }
                self
            }
        }

        let total_accum = (0..n)
            .into_par_iter()
            .fold(
                || CentroidAccumulator::new(k),
                |mut acc, i| {
                    let c = cluster_indices[i];
                    acc.add(c, &points[i]);
                    acc
                },
            )
            .reduce(|| CentroidAccumulator::new(k), CentroidAccumulator::combine);

        let mut center_moved = false;
        for (c, cluster) in clusters.iter_mut().enumerate().take(k) {
            if total_accum.sum_w[c] > 0.0 {
                let new_center = Lab::new(
                    total_accum.sum_l[c] / total_accum.sum_w[c],
                    total_accum.sum_a[c] / total_accum.sum_w[c],
                    total_accum.sum_b[c] / total_accum.sum_w[c],
                );
                if cluster.distance_squared(&new_center) > MIN_MOVEMENT_DISTANCE_SQ {
                    center_moved = true;
                }
                *cluster = new_center;
            }
        }

        if !center_moved {
            break;
        }
    }

    clusters
}

/// Quantizes points into cluster palettes using Wsmeans.
/// When the `parallel` feature is enabled and available, uses Rayon.
pub fn cluster_wsmeans(points: &[WeightedPoint], clusters: Vec<Lab>) -> Vec<Lab> {
    #[cfg(feature = "parallel")]
    {
        cluster_wsmeans_parallel(points, clusters)
    }
    #[cfg(not(feature = "parallel"))]
    {
        cluster_wsmeans_sequential(points, clusters)
    }
}

/// Convert pixel count frequency map to a slice of [`WeightedPoint`].
pub fn points_from_histogram(color_to_count: &HashMap<u32, u32>) -> Vec<WeightedPoint> {
    color_to_count
        .iter()
        .map(|(&argb, &weight)| WeightedPoint {
            lab: Argb::from_u32(argb).to_lab(),
            weight,
            argb,
        })
        .collect()
}
