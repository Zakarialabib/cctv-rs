//! Color quantization algorithms for video frames and detection crops.
//!
//! Provides the **Celebi Quantizer**, combining:
//! - Aspect-ratio preserving downsampling to $\le 112 \times 112$.
//! - Wu's variance-minimizing box partitioning.
//! - Parallelized Wsmeans (Weighted Square Means) clustering in CIE Lab space.

pub mod celebi;
pub mod color;
pub mod wsmeans;
pub mod wu;

pub use celebi::{QuantizerCelebi, MAX_IMAGE_DIMENSION};
pub use color::{Argb, Lab};
pub use wsmeans::{cluster_wsmeans, WeightedPoint};
pub use wu::QuantizerWu;
