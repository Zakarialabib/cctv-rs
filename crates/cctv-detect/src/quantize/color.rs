//! Color space conversions and utilities for quantization.

/// An sRGB color with 8-bit components packed as `0x00RRGGBB` or `0xAARRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Argb {
    /// Alpha channel (0..255).
    pub a: u8,
    /// Red channel (0..255).
    pub r: u8,
    /// Green channel (0..255).
    pub g: u8,
    /// Blue channel (0..255).
    pub b: u8,
}

impl Argb {
    /// Construct an ARGB color from individual u8 channels.
    pub const fn new(a: u8, r: u8, g: u8, b: u8) -> Self {
        Self { a, r, g, b }
    }

    /// Construct an opaque RGB color (alpha = 255).
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self { a: 255, r, g, b }
    }

    /// Construct from a 32-bit integer `0xAARRGGBB`.
    pub const fn from_u32(val: u32) -> Self {
        Self {
            a: ((val >> 24) & 0xFF) as u8,
            r: ((val >> 16) & 0xFF) as u8,
            g: ((val >> 8) & 0xFF) as u8,
            b: (val & 0xFF) as u8,
        }
    }

    /// Convert to a 32-bit integer `0xAARRGGBB`.
    pub const fn to_u32(self) -> u32 {
        ((self.a as u32) << 24) | ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    /// Convert to CIE L*a*b* color space.
    pub fn to_lab(self) -> Lab {
        Lab::from_rgb(self.r, self.g, self.b)
    }
}

/// CIE L*a*b* color representation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lab {
    /// Lightness (0.0 to 100.0).
    pub l: f64,
    /// Green-Red chromaticity coordinate.
    pub a: f64,
    /// Blue-Yellow chromaticity coordinate.
    pub b: f64,
}

impl Lab {
    /// Construct a Lab color point.
    pub const fn new(l: f64, a: f64, b: f64) -> Self {
        Self { l, a, b }
    }

    /// Convert sRGB [0..255] to CIE L*a*b*.
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        // Linearize sRGB
        let r_lin = srgb_to_linear(r as f64 / 255.0);
        let g_lin = srgb_to_linear(g as f64 / 255.0);
        let b_lin = srgb_to_linear(b as f64 / 255.0);

        // Convert linear RGB to XYZ (D65 standard observer)
        let x = r_lin * 0.4124564 + g_lin * 0.3575761 + b_lin * 0.1804375;
        let y = r_lin * 0.2126729 + g_lin * 0.7151522 + b_lin * 0.0721750;
        let z = r_lin * 0.0193339 + g_lin * 0.1191920 + b_lin * 0.9503041;

        // D65 reference white point
        let x_n = 0.95047;
        let y_n = 1.00000;
        let z_n = 1.08883;

        let fx = f_lab(x / x_n);
        let fy = f_lab(y / y_n);
        let fz = f_lab(z / z_n);

        let l = (116.0 * fy - 16.0).clamp(0.0, 100.0);
        let a = 500.0 * (fx - fy);
        let b_val = 200.0 * (fy - fz);

        Self { l, a, b: b_val }
    }

    /// Convert CIE L*a*b* back to sRGB.
    pub fn to_argb(self) -> Argb {
        let fy = (self.l + 16.0) / 116.0;
        let fx = self.a / 500.0 + fy;
        let fz = fy - self.b / 200.0;

        let x_n = 0.95047;
        let y_n = 1.00000;
        let z_n = 1.08883;

        let x = x_n * f_inv_lab(fx);
        let y = y_n * f_inv_lab(fy);
        let z = z_n * f_inv_lab(fz);

        // Convert XYZ to linear RGB
        let r_lin = x * 3.2404542 + y * -1.5371385 + z * -0.4985314;
        let g_lin = x * -0.9692660 + y * 1.8760108 + z * 0.0415560;
        let b_lin = x * 0.0556434 + y * -0.2040259 + z * 1.0572252;

        let r = (linear_to_srgb(r_lin) * 255.0).round().clamp(0.0, 255.0) as u8;
        let g = (linear_to_srgb(g_lin) * 255.0).round().clamp(0.0, 255.0) as u8;
        let b = (linear_to_srgb(b_lin) * 255.0).round().clamp(0.0, 255.0) as u8;

        Argb::from_rgb(r, g, b)
    }

    /// Squared Euclidean distance in CIE Lab space.
    #[inline]
    pub fn distance_squared(&self, other: &Self) -> f64 {
        let dl = self.l - other.l;
        let da = self.a - other.a;
        let db = self.b - other.b;
        dl * dl + da * da + db * db
    }

    /// Euclidean distance in CIE Lab space.
    #[inline]
    pub fn distance(&self, other: &Self) -> f64 {
        self.distance_squared(other).sqrt()
    }
}

#[inline]
fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[inline]
fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.0031308 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

#[inline]
fn f_lab(t: f64) -> f64 {
    let delta = 6.0 / 29.0;
    if t > delta * delta * delta {
        t.cbrt()
    } else {
        t / (3.0 * delta * delta) + 4.0 / 29.0
    }
}

#[inline]
fn f_inv_lab(t: f64) -> f64 {
    let delta = 6.0 / 29.0;
    if t > delta {
        t * t * t
    } else {
        3.0 * delta * delta * (t - 4.0 / 29.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgb_lab_roundtrip() {
        let colors = [
            (255, 0, 0),
            (0, 255, 0),
            (0, 0, 255),
            (255, 255, 255),
            (0, 0, 0),
            (128, 128, 128),
            (240, 180, 50),
        ];

        for (r, g, b) in colors {
            let orig = Argb::from_rgb(r, g, b);
            let lab = orig.to_lab();
            let roundtrip = lab.to_argb();
            assert!(
                (orig.r as i16 - roundtrip.r as i16).abs() <= 1,
                "R mismatch for {r},{g},{b}"
            );
            assert!(
                (orig.g as i16 - roundtrip.g as i16).abs() <= 1,
                "G mismatch for {r},{g},{b}"
            );
            assert!(
                (orig.b as i16 - roundtrip.b as i16).abs() <= 1,
                "B mismatch for {r},{g},{b}"
            );
        }
    }
}
