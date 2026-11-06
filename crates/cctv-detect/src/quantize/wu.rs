//! Wu's Color Quantizer (Graphics Gems II).
//!
//! Partitions the 3D RGB color histogram into boxes with minimal variance.

use std::collections::HashMap;

const INDEX_BITS: usize = 5;
const INDEX_COUNT: usize = (1 << INDEX_BITS) + 1; // 33
const TOTAL_SIZE: usize = INDEX_COUNT * INDEX_COUNT * INDEX_COUNT; // 35937

#[inline]
fn get_index(r: usize, g: usize, b: usize) -> usize {
    (r << (INDEX_BITS * 2)) + (r << (INDEX_BITS + 1)) + r + (g << INDEX_BITS) + g + b
}

/// A 3D bounding box in the 5-bit color space.
#[derive(Debug, Clone, Copy, Default)]
struct Box {
    r0: usize,
    r1: usize,
    g0: usize,
    g1: usize,
    b0: usize,
    b1: usize,
    vol: usize,
}

/// Result of Wu quantization.
#[derive(Debug, Clone)]
pub struct QuantizedColor {
    /// ARGB color value (0xAARRGGBB).
    pub color: u32,
    /// Number of pixels represented by this color cluster.
    pub count: u32,
}

/// Wu's color quantization algorithm.
pub struct QuantizerWu {
    weights: Vec<f64>,
    moments_r: Vec<f64>,
    moments_g: Vec<f64>,
    moments_b: Vec<f64>,
    moments: Vec<f64>,
    cubes: Vec<Box>,
}

impl QuantizerWu {
    /// Create a new Wu quantizer instance.
    pub fn new() -> Self {
        Self {
            weights: vec![0.0; TOTAL_SIZE],
            moments_r: vec![0.0; TOTAL_SIZE],
            moments_g: vec![0.0; TOTAL_SIZE],
            moments_b: vec![0.0; TOTAL_SIZE],
            moments: vec![0.0; TOTAL_SIZE],
            cubes: Vec::new(),
        }
    }

    /// Quantize a histogram of ARGB pixel counts to at most `max_colors`.
    pub fn quantize(&mut self, color_to_count: &HashMap<u32, u32>, max_colors: usize) -> Vec<u32> {
        if color_to_count.is_empty() || max_colors == 0 {
            return Vec::new();
        }

        if color_to_count.len() <= max_colors {
            return color_to_count.keys().copied().collect();
        }

        self.construct_histogram(color_to_count);
        self.compute_moments();

        let mut next = 0;
        let mut volume_variance = vec![0.0; max_colors];

        self.cubes = vec![Box::default(); max_colors];
        self.cubes[0] = Box {
            r0: 0,
            r1: INDEX_COUNT - 1,
            g0: 0,
            g1: INDEX_COUNT - 1,
            b0: 0,
            b1: INDEX_COUNT - 1,
            vol: 0,
        };

        for k in 1..max_colors {
            let mut set1 = self.cubes[next];
            let mut set2 = Box::default();
            if self.cut(&mut set1, &mut set2) {
                self.cubes[next] = set1;
                self.cubes[k] = set2;
                volume_variance[next] = if self.cubes[next].vol > 1 {
                    self.variance(&self.cubes[next])
                } else {
                    0.0
                };
                volume_variance[k] = if self.cubes[k].vol > 1 {
                    self.variance(&self.cubes[k])
                } else {
                    0.0
                };
            } else {
                volume_variance[next] = 0.0;
                // cannot cut further
            }

            // Find next cube with largest variance
            next = 0;
            let mut temp = volume_variance[0];
            for (i, &var) in volume_variance.iter().enumerate().take(k + 1) {
                if var > temp {
                    temp = var;
                    next = i;
                }
            }
            if temp <= 0.0 {
                // Done splitting
                self.cubes.truncate(k + 1);
                break;
            }
        }

        let mut colors = Vec::with_capacity(self.cubes.len());
        for cube in &self.cubes {
            let weight = self.volume(cube, &self.weights);
            if weight > 0.0 {
                let r = ((self.volume(cube, &self.moments_r) / weight).round() as u32).min(255);
                let g = ((self.volume(cube, &self.moments_g) / weight).round() as u32).min(255);
                let b = ((self.volume(cube, &self.moments_b) / weight).round() as u32).min(255);
                colors.push(0xFF00_0000 | (r << 16) | (g << 8) | b);
            }
        }

        colors
    }

    fn construct_histogram(&mut self, color_to_count: &HashMap<u32, u32>) {
        self.weights.fill(0.0);
        self.moments_r.fill(0.0);
        self.moments_g.fill(0.0);
        self.moments_b.fill(0.0);
        self.moments.fill(0.0);

        for (&pixel, &count) in color_to_count {
            let r = ((pixel >> 16) & 0xFF) as usize;
            let g = ((pixel >> 8) & 0xFF) as usize;
            let b = (pixel & 0xFF) as usize;

            let i_r = (r >> (8 - INDEX_BITS)) + 1;
            let i_g = (g >> (8 - INDEX_BITS)) + 1;
            let i_b = (b >> (8 - INDEX_BITS)) + 1;

            let index = get_index(i_r, i_g, i_b);
            let cnt = count as f64;
            self.weights[index] += cnt;
            self.moments_r[index] += cnt * (r as f64);
            self.moments_g[index] += cnt * (g as f64);
            self.moments_b[index] += cnt * (b as f64);
            self.moments[index] += cnt * ((r * r + g * g + b * b) as f64);
        }
    }

    fn compute_moments(&mut self) {
        let mut area = [0.0; INDEX_COUNT];
        let mut area_r = [0.0; INDEX_COUNT];
        let mut area_g = [0.0; INDEX_COUNT];
        let mut area_b = [0.0; INDEX_COUNT];
        let mut area2 = [0.0; INDEX_COUNT];

        for r in 1..INDEX_COUNT {
            area.fill(0.0);
            area_r.fill(0.0);
            area_g.fill(0.0);
            area_b.fill(0.0);
            area2.fill(0.0);

            for g in 1..INDEX_COUNT {
                let mut line = 0.0;
                let mut line_r = 0.0;
                let mut line_g = 0.0;
                let mut line_b = 0.0;
                let mut line2 = 0.0;

                for b in 1..INDEX_COUNT {
                    let ind1 = get_index(r, g, b);
                    line += self.weights[ind1];
                    line_r += self.moments_r[ind1];
                    line_g += self.moments_g[ind1];
                    line_b += self.moments_b[ind1];
                    line2 += self.moments[ind1];

                    area[b] += line;
                    area_r[b] += line_r;
                    area_g[b] += line_g;
                    area_b[b] += line_b;
                    area2[b] += line2;

                    let ind2 = get_index(r - 1, g, b);
                    self.weights[ind1] = self.weights[ind2] + area[b];
                    self.moments_r[ind1] = self.moments_r[ind2] + area_r[b];
                    self.moments_g[ind1] = self.moments_g[ind2] + area_g[b];
                    self.moments_b[ind1] = self.moments_b[ind2] + area_b[b];
                    self.moments[ind1] = self.moments[ind2] + area2[b];
                }
            }
        }
    }

    fn volume(&self, cube: &Box, moment: &[f64]) -> f64 {
        moment[get_index(cube.r1, cube.g1, cube.b1)]
            - moment[get_index(cube.r1, cube.g1, cube.b0)]
            - moment[get_index(cube.r1, cube.g0, cube.b1)]
            + moment[get_index(cube.r1, cube.g0, cube.b0)]
            - moment[get_index(cube.r0, cube.g1, cube.b1)]
            + moment[get_index(cube.r0, cube.g1, cube.b0)]
            + moment[get_index(cube.r0, cube.g0, cube.b1)]
            - moment[get_index(cube.r0, cube.g0, cube.b0)]
    }

    fn bottom(&self, cube: &Box, dir: u8, moment: &[f64]) -> f64 {
        match dir {
            0 => {
                // RED
                -moment[get_index(cube.r0, cube.g1, cube.b1)]
                    + moment[get_index(cube.r0, cube.g1, cube.b0)]
                    + moment[get_index(cube.r0, cube.g0, cube.b1)]
                    - moment[get_index(cube.r0, cube.g0, cube.b0)]
            }
            1 => {
                // GREEN
                -moment[get_index(cube.r1, cube.g0, cube.b1)]
                    + moment[get_index(cube.r1, cube.g0, cube.b0)]
                    + moment[get_index(cube.r0, cube.g0, cube.b1)]
                    - moment[get_index(cube.r0, cube.g0, cube.b0)]
            }
            2 => {
                // BLUE
                -moment[get_index(cube.r1, cube.g1, cube.b0)]
                    + moment[get_index(cube.r1, cube.g0, cube.b0)]
                    + moment[get_index(cube.r0, cube.g1, cube.b0)]
                    - moment[get_index(cube.r0, cube.g0, cube.b0)]
            }
            _ => 0.0,
        }
    }

    fn top(&self, cube: &Box, dir: u8, pos: usize, moment: &[f64]) -> f64 {
        match dir {
            0 => {
                moment[get_index(pos, cube.g1, cube.b1)]
                    - moment[get_index(pos, cube.g1, cube.b0)]
                    - moment[get_index(pos, cube.g0, cube.b1)]
                    + moment[get_index(pos, cube.g0, cube.b0)]
            }
            1 => {
                moment[get_index(cube.r1, pos, cube.b1)]
                    - moment[get_index(cube.r1, pos, cube.b0)]
                    - moment[get_index(cube.r0, pos, cube.b1)]
                    + moment[get_index(cube.r0, pos, cube.b0)]
            }
            2 => {
                moment[get_index(cube.r1, cube.g1, pos)]
                    - moment[get_index(cube.r1, cube.g0, pos)]
                    - moment[get_index(cube.r0, cube.g1, pos)]
                    + moment[get_index(cube.r0, cube.g0, pos)]
            }
            _ => 0.0,
        }
    }

    fn variance(&self, cube: &Box) -> f64 {
        let dr = self.volume(cube, &self.moments_r);
        let dg = self.volume(cube, &self.moments_g);
        let db = self.volume(cube, &self.moments_b);
        let xx = self.volume(cube, &self.moments);
        let hypotenuse = dr * dr + dg * dg + db * db;
        let volume = self.volume(cube, &self.weights);
        if volume > 0.0 {
            xx - hypotenuse / volume
        } else {
            0.0
        }
    }

    fn cut(&self, set1: &mut Box, set2: &mut Box) -> bool {
        let whole_r = self.volume(set1, &self.moments_r);
        let whole_g = self.volume(set1, &self.moments_g);
        let whole_b = self.volume(set1, &self.moments_b);
        let whole_w = self.volume(set1, &self.weights);

        let (maxr, cutr) = self.maximize(
            set1,
            0,
            set1.r0 + 1,
            set1.r1,
            whole_r,
            whole_g,
            whole_b,
            whole_w,
        );
        let (maxg, cutg) = self.maximize(
            set1,
            1,
            set1.g0 + 1,
            set1.g1,
            whole_r,
            whole_g,
            whole_b,
            whole_w,
        );
        let (maxb, cutb) = self.maximize(
            set1,
            2,
            set1.b0 + 1,
            set1.b1,
            whole_r,
            whole_g,
            whole_b,
            whole_w,
        );

        let dir = if maxr >= maxg && maxr >= maxb {
            if cutr < 0 {
                return false;
            }
            0
        } else if maxg >= maxr && maxg >= maxb {
            if cutg < 0 {
                return false;
            }
            1
        } else {
            if cutb < 0 {
                return false;
            }
            2
        };

        *set2 = *set1;

        match dir {
            0 => {
                set2.r0 = cutr as usize;
                set1.r1 = set2.r0;
            }
            1 => {
                set2.g0 = cutg as usize;
                set1.g1 = set2.g0;
            }
            2 => {
                set2.b0 = cutb as usize;
                set1.b1 = set2.b0;
            }
            _ => unreachable!(),
        }

        set1.vol = (set1.r1 - set1.r0) * (set1.g1 - set1.g0) * (set1.b1 - set1.b0);
        set2.vol = (set2.r1 - set2.r0) * (set2.g1 - set2.g0) * (set2.b1 - set2.b0);

        true
    }

    #[allow(clippy::too_many_arguments)]
    fn maximize(
        &self,
        cube: &Box,
        dir: u8,
        first: usize,
        last: usize,
        whole_r: f64,
        whole_g: f64,
        whole_b: f64,
        whole_w: f64,
    ) -> (f64, i32) {
        let base_r = self.bottom(cube, dir, &self.moments_r);
        let base_g = self.bottom(cube, dir, &self.moments_g);
        let base_b = self.bottom(cube, dir, &self.moments_b);
        let base_w = self.bottom(cube, dir, &self.weights);

        let mut max = 0.0;
        let mut cut = -1;

        for i in first..last {
            let half_r = base_r + self.top(cube, dir, i, &self.moments_r);
            let half_g = base_g + self.top(cube, dir, i, &self.moments_g);
            let half_b = base_b + self.top(cube, dir, i, &self.moments_b);
            let half_w = base_w + self.top(cube, dir, i, &self.weights);

            if half_w == 0.0 {
                continue;
            }

            let mut temp = (half_r * half_r + half_g * half_g + half_b * half_b) / half_w;

            let rem_r = whole_r - half_r;
            let rem_g = whole_g - half_g;
            let rem_b = whole_b - half_b;
            let rem_w = whole_w - half_w;

            if rem_w == 0.0 {
                continue;
            }

            temp += (rem_r * rem_r + rem_g * rem_g + rem_b * rem_b) / rem_w;

            if temp > max {
                max = temp;
                cut = i as i32;
            }
        }

        (max, cut)
    }
}

impl Default for QuantizerWu {
    fn default() -> Self {
        Self::new()
    }
}
