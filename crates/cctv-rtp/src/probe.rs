//! Media probing and descriptor generation for streams and file containers.
//!
//! Provides [`MediaProbe`], [`MediaDescriptor`], and [`TranscodePlan`] to inspect
//! live Annex-B elementary streams and container/asset byte streams (e.g. MP4 avcC/hvcC boxes).

use crate::annexb::{parse_h264_sps, parse_h265_sps, probe_annexb, VideoCodec};
use crate::error::RtpError;

/// High-level descriptor of a media stream or file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaDescriptor {
    /// Coded video standard.
    pub codec: VideoCodec,
    /// Profile identifier.
    pub profile: u8,
    /// Level identifier.
    pub level: u8,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Transcoding and processing plan derived from a media descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscodePlan {
    /// Source media descriptor.
    pub descriptor: MediaDescriptor,
    /// Whether transcoding is required to reach a baseline configuration.
    pub passthrough_possible: bool,
    /// Target width.
    pub target_width: u32,
    /// Target height.
    pub target_height: u32,
}

impl TranscodePlan {
    /// Construct a plan for the given source descriptor.
    pub fn for_descriptor(descriptor: MediaDescriptor) -> Self {
        let passthrough_possible = descriptor.width <= 1920 && descriptor.height <= 1080;
        let target_width = descriptor.width.min(1920);
        let target_height = descriptor.height.min(1080);
        Self {
            descriptor,
            passthrough_possible,
            target_width,
            target_height,
        }
    }
}

/// Unified media probe capable of probing elementary streams as well as file containers.
pub struct MediaProbe;

impl MediaProbe {
    /// Probes a live Annex-B elementary stream.
    ///
    /// Produces a [`MediaDescriptor`] by locating and decoding sequence parameter sets (SPS/VPS).
    pub fn probe_stream(bytes: &[u8]) -> Result<MediaDescriptor, RtpError> {
        let params = probe_annexb(bytes)?;
        Ok(MediaDescriptor {
            codec: params.codec,
            profile: params.profile,
            level: params.level,
            width: params.width,
            height: params.height,
        })
    }

    /// Probes arbitrary bytes, handling both file containers (e.g., MP4 avcC / hvcC boxes)
    /// and raw Annex-B byte streams.
    pub fn probe_bytes(bytes: &[u8]) -> Result<MediaDescriptor, RtpError> {
        if bytes.is_empty() {
            return Err(RtpError::Incomplete);
        }

        // Check if this is an MP4/ISOBMFF container or raw box by looking for 'avcC' or 'hvcC'
        if let Some(desc) = probe_isobmff_boxes(bytes) {
            return Ok(desc);
        }

        // Fallback to elementary stream probe
        Self::probe_stream(bytes)
    }
}

/// Searches for and parses ISO/IEC 14496-15 `avcC` or `hvcC` configuration boxes.
fn probe_isobmff_boxes(bytes: &[u8]) -> Option<MediaDescriptor> {
    // Search for "avcC" box
    if let Some(pos) = find_subsequence(bytes, b"avcC") {
        if pos >= 4 {
            let box_payload = &bytes[pos + 4..];
            if let Some(desc) = parse_avcc_box(box_payload) {
                return Some(desc);
            }
        }
    }

    // Search for "hvcC" box
    if let Some(pos) = find_subsequence(bytes, b"hvcC") {
        if pos >= 4 {
            let box_payload = &bytes[pos + 4..];
            if let Some(desc) = parse_hvcc_box(box_payload) {
                return Some(desc);
            }
        }
    }

    None
}

/// Parses an ISO/IEC 14496-15 AVCDecoderConfigurationRecord (`avcC`).
///
/// Byte layout:
/// - byte 0: configurationVersion (1)
/// - byte 1: AVCProfileIndication
/// - byte 2: profile_compatibility
/// - byte 3: AVCLevelIndication
/// - byte 4: lengthSizeMinusOne (6 reserved bits + 2 bits)
/// - byte 5: numOfSequenceParameterSets (3 reserved bits + 5 bits)
/// - followed by [sps_length (2 bytes)] [sps_bytes...]
fn parse_avcc_box(payload: &[u8]) -> Option<MediaDescriptor> {
    if payload.len() < 7 {
        return None;
    }
    let profile = payload[1];
    let level = payload[3];
    let num_sps = payload[5] & 0x1F;
    if num_sps == 0 {
        return None;
    }

    let mut offset = 6;
    if offset + 2 > payload.len() {
        return None;
    }
    let sps_len = u16::from_be_bytes([payload[offset], payload[offset + 1]]) as usize;
    offset += 2;

    if offset + sps_len > payload.len() {
        return None;
    }
    let sps_nal = &payload[offset..offset + sps_len];
    let params = parse_h264_sps(sps_nal)?;

    Some(MediaDescriptor {
        codec: VideoCodec::H264,
        profile,
        level,
        width: params.width,
        height: params.height,
    })
}

/// Parses an ISO/IEC 14496-15 HEVCDecoderConfigurationRecord (`hvcC`).
///
/// Byte layout:
/// - byte 0: configurationVersion (1)
/// - byte 1: general_profile_space (2 bits), general_tier_flag (1 bit), general_profile_idc (5 bits)
/// - bytes 2..6: general_profile_compatibility_flags (32 bits)
/// - bytes 6..12: general_constraint_indicator_flags (48 bits)
/// - byte 12: general_level_idc
/// - ... array of NALUs (VPS/SPS/PPS)
fn parse_hvcc_box(payload: &[u8]) -> Option<MediaDescriptor> {
    if payload.len() < 23 {
        return None;
    }
    let profile = payload[1] & 0x1F;
    let level = payload[12];

    let num_of_arrays = payload[22] as usize;
    let mut offset = 23;
    let mut parsed_params = None;

    for _ in 0..num_of_arrays {
        if offset + 3 > payload.len() {
            break;
        }
        let nal_type = payload[offset] & 0x3F;
        let num_nalus = u16::from_be_bytes([payload[offset + 1], payload[offset + 2]]) as usize;
        offset += 3;

        for _ in 0..num_nalus {
            if offset + 2 > payload.len() {
                break;
            }
            let nal_len = u16::from_be_bytes([payload[offset], payload[offset + 1]]) as usize;
            offset += 2;
            if offset + nal_len > payload.len() {
                break;
            }
            let nal_bytes = &payload[offset..offset + nal_len];
            if nal_type == 33 {
                // SPS
                if let Some(params) = parse_h265_sps(nal_bytes) {
                    parsed_params = Some(params);
                }
            }
            offset += nal_len;
        }
    }

    parsed_params.map(|params| MediaDescriptor {
        codec: VideoCodec::H265,
        profile,
        level,
        width: params.width,
        height: params.height,
    })
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
