//! Shared payload types.

/// One decodable access unit: the NAL units belonging to a single frame.
///
/// Produced by both [`crate::h264::H264Depacketizer`] and
/// [`crate::h265::H265Depacketizer`], so a caller can treat a codec's output
/// uniformly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessUnit {
    /// The RTP timestamp every packet of this unit shared.
    pub timestamp: u32,
    /// The NAL units, in wire order, with their headers intact.
    pub nal_units: Vec<Vec<u8>>,
    /// Total bytes of NAL data.
    pub bytes: usize,
}
