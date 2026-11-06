//! Annex-B elementary-stream parsing and SPS/PPS/VPS parameter extraction.
//!
//! Handles Annex-B byte-stream framing (0x000001 / 0x00000001 start codes),
//! emulation prevention byte stripping (0x000003 -> 0x0000), and minimal
//! exp-Golomb inspection of H.264 SPS and H.265 SPS/VPS to extract:
//! - Codec (H.264 vs H.265 / HEVC)
//! - Profile & Level
//! - Resolution (width & height with cropping accounted for).

use crate::error::RtpError;

/// Video codec identification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoCodec {
    /// H.264 / AVC
    H264,
    /// H.265 / HEVC
    H265,
}

/// Profile, level, and resolution parameters parsed from sequence headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamParameters {
    /// Video codec.
    pub codec: VideoCodec,
    /// Profile IDC / indication.
    pub profile: u8,
    /// Level IDC / indication.
    pub level: u8,
    /// Coded frame width in pixels (after cropping).
    pub width: u32,
    /// Coded frame height in pixels (after cropping).
    pub height: u32,
}

/// Bit reader with exp-Golomb coding support.
#[derive(Debug)]
pub struct BitReader<'a> {
    data: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    /// Create a new BitReader over a slice.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, bit_pos: 0 }
    }

    /// Number of bits remaining.
    pub fn remaining_bits(&self) -> usize {
        (self.data.len() * 8).saturating_sub(self.bit_pos)
    }

    /// Read a single bit, returning `None` if EOF.
    pub fn read_bit(&mut self) -> Option<u8> {
        let byte_index = self.bit_pos / 8;
        if byte_index >= self.data.len() {
            return None;
        }
        let bit_index = 7 - (self.bit_pos % 8);
        self.bit_pos += 1;
        Some((self.data[byte_index] >> bit_index) & 1)
    }

    /// Read up to 32 bits.
    pub fn read_bits(&mut self, n: usize) -> Option<u32> {
        if n == 0 {
            return Some(0);
        }
        if n > 32 || self.remaining_bits() < n {
            return None;
        }
        let mut val = 0u32;
        for _ in 0..n {
            val = (val << 1) | u32::from(self.read_bit()?);
        }
        Some(val)
    }

    /// Read an unsigned exponential-Golomb code (ue(v)).
    pub fn read_ue(&mut self) -> Option<u32> {
        let mut leading_zeros = 0usize;
        while self.read_bit()? == 0 {
            leading_zeros += 1;
            if leading_zeros > 31 {
                return None;
            }
        }
        if leading_zeros == 0 {
            return Some(0);
        }
        let suffix = self.read_bits(leading_zeros)?;
        Some((1u32 << leading_zeros) - 1 + suffix)
    }

    /// Read a signed exponential-Golomb code (se(v)).
    pub fn read_se(&mut self) -> Option<i32> {
        let code = self.read_ue()?;
        if code % 2 == 1 {
            Some(i32::try_from(code.div_ceil(2)).ok()?)
        } else {
            Some(-i32::try_from(code / 2).ok()?)
        }
    }

    /// Skip `n` bits.
    pub fn skip_bits(&mut self, n: usize) -> Option<()> {
        if self.remaining_bits() < n {
            None
        } else {
            self.bit_pos += n;
            Some(())
        }
    }
}

/// Strips emulation prevention bytes (0x00 0x00 0x03) to obtain the Raw Byte Sequence Payload (RBSP).
pub fn unescape_rbsp(data: &[u8]) -> Vec<u8> {
    let mut rbsp = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if i + 2 < data.len() && data[i] == 0x00 && data[i + 1] == 0x00 && data[i + 2] == 0x03 {
            rbsp.push(0x00);
            rbsp.push(0x00);
            i += 3;
        } else {
            rbsp.push(data[i]);
            i += 1;
        }
    }
    rbsp
}

/// Splits an Annex-B elementary stream into individual raw NAL units (excluding start codes).
pub fn split_annexb(stream: &[u8]) -> Vec<&[u8]> {
    let mut nals = Vec::new();
    let mut start_indices = Vec::new();

    let mut i = 0;
    while i + 2 < stream.len() {
        if stream[i] == 0 && stream[i + 1] == 0 {
            if stream[i + 2] == 1 {
                // 3-byte start code 00 00 01
                let start_code_len = if i > 0 && stream[i - 1] == 0 { 4 } else { 3 };
                let nal_start = i + 3;
                start_indices.push((nal_start - start_code_len, nal_start));
                i += 3;
                continue;
            } else if i + 3 < stream.len() && stream[i + 2] == 0 && stream[i + 3] == 1 {
                // 4-byte start code 00 00 00 01
                let nal_start = i + 4;
                start_indices.push((i, nal_start));
                i += 4;
                continue;
            }
        }
        i += 1;
    }

    for idx in 0..start_indices.len() {
        let nal_start = start_indices[idx].1;
        let nal_end = if idx + 1 < start_indices.len() {
            start_indices[idx + 1].0
        } else {
            stream.len()
        };
        if nal_start <= nal_end {
            let nal = &stream[nal_start..nal_end];
            if !nal.is_empty() {
                nals.push(nal);
            }
        }
    }

    nals
}

/// Parses an H.264 SPS NAL unit and returns its stream parameters.
pub fn parse_h264_sps(sps_nal: &[u8]) -> Option<StreamParameters> {
    if sps_nal.len() < 4 {
        return None;
    }
    let nal_type = sps_nal[0] & 0x1F;
    if nal_type != 7 {
        return None; // Not SPS
    }

    let profile = sps_nal[1];
    let level = sps_nal[3];

    // Unescape RBSP starting from byte 1 (skip NAL header)
    let rbsp = unescape_rbsp(&sps_nal[1..]);
    // sps_nal[1] is profile_idc, [2] is constraint flags, [3] is level_idc
    if rbsp.len() < 3 {
        return None;
    }
    let mut reader = BitReader::new(&rbsp[3..]); // After profile, constraints, level

    let _seq_param_set_id = reader.read_ue()?;

    let chroma_format_idc = if matches!(
        profile,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        let idc = reader.read_ue()?;
        if idc == 3 {
            reader.read_bit()?; // separate_colour_plane_flag
        }
        reader.read_ue()?; // bit_depth_luma_minus8
        reader.read_ue()?; // bit_depth_chroma_minus8
        reader.read_bit()?; // qpprime_y_zero_transform_bypass_flag
        if reader.read_bit()? == 1 {
            // seq_scaling_matrix_present_flag
            let count = if idc == 3 { 12 } else { 8 };
            for _ in 0..count {
                if reader.read_bit()? == 1 {
                    // seq_scaling_list_present_flag
                    let size = 16;
                    let mut last = 8i32;
                    let mut next = 8i32;
                    for _ in 0..size {
                        if next != 0 {
                            let delta = reader.read_se()?;
                            next = (last + delta + 256) % 256;
                        }
                        last = if next != 0 { next } else { last };
                    }
                }
            }
        }
        idc
    } else {
        1 // Default 4:2:0
    };

    let _log2_max_frame_num_minus4 = reader.read_ue()?;
    let pic_order_cnt_type = reader.read_ue()?;
    if pic_order_cnt_type == 0 {
        let _log2_max_pic_order_cnt_lsb_minus4 = reader.read_ue()?;
    } else if pic_order_cnt_type == 1 {
        let _delta_pic_order_always_zero_flag = reader.read_bit()?;
        let _offset_for_non_ref_pic = reader.read_se()?;
        let _offset_for_top_to_bottom_field = reader.read_se()?;
        let num_ref_frames_in_pic_order_cnt_cycle = reader.read_ue()?;
        for _ in 0..num_ref_frames_in_pic_order_cnt_cycle {
            let _ = reader.read_se()?;
        }
    }

    let _max_num_ref_frames = reader.read_ue()?;
    let _gaps_in_frame_num_value_allowed_flag = reader.read_bit()?;
    let pic_width_in_mbs_minus1 = reader.read_ue()?;
    let pic_height_in_map_units_minus1 = reader.read_ue()?;
    let frame_mbs_only_flag = reader.read_bit()?;
    if frame_mbs_only_flag == 0 {
        let _mb_adaptive_frame_field_flag = reader.read_bit()?;
    }
    let _direct_8x8_inference_flag = reader.read_bit()?;

    let frame_cropping_flag = reader.read_bit()?;
    let (crop_left, crop_right, crop_top, crop_bottom) = if frame_cropping_flag == 1 {
        (
            reader.read_ue()?,
            reader.read_ue()?,
            reader.read_ue()?,
            reader.read_ue()?,
        )
    } else {
        (0, 0, 0, 0)
    };

    let raw_width = (pic_width_in_mbs_minus1 + 1) * 16;
    let raw_height =
        (pic_height_in_map_units_minus1 + 1) * 16 * (2 - u32::from(frame_mbs_only_flag));

    let crop_unit_x = if chroma_format_idc == 0 { 1 } else { 2 };
    let crop_unit_y = if chroma_format_idc == 0 {
        2 - u32::from(frame_mbs_only_flag)
    } else {
        2 * (2 - u32::from(frame_mbs_only_flag))
    };

    let width = raw_width.saturating_sub(crop_unit_x * (crop_left + crop_right));
    let height = raw_height.saturating_sub(crop_unit_y * (crop_top + crop_bottom));

    Some(StreamParameters {
        codec: VideoCodec::H264,
        profile,
        level,
        width,
        height,
    })
}

/// Parses an H.265 / HEVC SPS NAL unit and returns its stream parameters.
pub fn parse_h265_sps(sps_nal: &[u8]) -> Option<StreamParameters> {
    if sps_nal.len() < 3 {
        return None;
    }
    let nal_type = (sps_nal[0] >> 1) & 0x3F;
    if nal_type != 33 {
        return None; // Not HEVC SPS
    }

    let rbsp = unescape_rbsp(&sps_nal[2..]);
    let mut reader = BitReader::new(&rbsp);

    let _sps_video_parameter_set_id = reader.read_bits(4)?;
    let sps_max_sub_layers_minus1 = reader.read_bits(3)? as usize;
    let _sps_temporal_id_nesting_flag = reader.read_bit()?;

    // profile_tier_level(profilePresentFlag = 1, maxNumSubLayersMinus1 = sps_max_sub_layers_minus1)
    let _general_profile_space = reader.read_bits(2)?;
    let _general_tier_flag = reader.read_bit()?;
    let profile = reader.read_bits(5)? as u8;
    let _general_profile_compatibility_flags = reader.read_bits(32)?;
    let _general_progressive_source_flag = reader.read_bit()?;
    let _general_interlaced_source_flag = reader.read_bit()?;
    let _general_non_packed_constraint_flag = reader.read_bit()?;
    let _general_frame_only_constraint_flag = reader.read_bit()?;
    reader.skip_bits(44)?; // reserved / flags
    let level = reader.read_bits(8)? as u8;

    let mut sub_layer_profile_present_flags = Vec::with_capacity(sps_max_sub_layers_minus1);
    let mut sub_layer_level_present_flags = Vec::with_capacity(sps_max_sub_layers_minus1);
    for _ in 0..sps_max_sub_layers_minus1 {
        sub_layer_profile_present_flags.push(reader.read_bit()?);
        sub_layer_level_present_flags.push(reader.read_bit()?);
    }
    if sps_max_sub_layers_minus1 > 0 {
        for _ in sps_max_sub_layers_minus1..8 {
            reader.skip_bits(2)?; // reserved_zero_2bits
        }
    }
    for i in 0..sps_max_sub_layers_minus1 {
        if sub_layer_profile_present_flags.get(i).copied() == Some(1) {
            reader.skip_bits(32 + 32 + 24)?; // profile space, tier, profile, flags, constraints
        }
        if sub_layer_level_present_flags.get(i).copied() == Some(1) {
            reader.skip_bits(8)?; // sub_layer_level_idc
        }
    }

    let _sps_seq_parameter_set_id = reader.read_ue()?;
    let chroma_format_idc = reader.read_ue()?;
    if chroma_format_idc == 3 {
        reader.read_bit()?; // separate_colour_plane_flag
    }
    let pic_width_in_luma_samples = reader.read_ue()?;
    let pic_height_in_luma_samples = reader.read_ue()?;

    let conformance_window_flag = reader.read_bit()?;
    let (conf_win_left_offset, conf_win_right_offset, conf_win_top_offset, conf_win_bottom_offset) =
        if conformance_window_flag == 1 {
            (
                reader.read_ue()?,
                reader.read_ue()?,
                reader.read_ue()?,
                reader.read_ue()?,
            )
        } else {
            (0, 0, 0, 0)
        };

    let (sub_width_c, sub_height_c) = match chroma_format_idc {
        1 => (2u32, 2u32), // 4:2:0
        2 => (2u32, 1u32), // 4:2:2
        3 => (1u32, 1u32), // 4:4:4
        _ => (1u32, 1u32), // monochrome or unknown
    };

    let width = pic_width_in_luma_samples
        .saturating_sub(sub_width_c * (conf_win_left_offset + conf_win_right_offset));
    let height = pic_height_in_luma_samples
        .saturating_sub(sub_height_c * (conf_win_top_offset + conf_win_bottom_offset));

    Some(StreamParameters {
        codec: VideoCodec::H265,
        profile,
        level,
        width,
        height,
    })
}

/// Parses an Annex-B elementary stream to extract sequence stream parameters.
pub fn probe_annexb(bytes: &[u8]) -> Result<StreamParameters, RtpError> {
    if bytes.is_empty() {
        return Err(RtpError::Incomplete);
    }

    let nals = split_annexb(bytes);
    if nals.is_empty() {
        return Err(RtpError::Incomplete);
    }

    // Try H.264 SPS first (NAL type 7)
    for nal in &nals {
        if !nal.is_empty() && (nal[0] & 0x1F) == 7 {
            if let Some(params) = parse_h264_sps(nal) {
                return Ok(params);
            }
        }
    }

    // Try H.265 SPS (NAL type 33)
    for nal in &nals {
        if nal.len() >= 2 {
            let nal_type = (nal[0] >> 1) & 0x3F;
            if nal_type == 33 {
                if let Some(params) = parse_h265_sps(nal) {
                    return Ok(params);
                }
            }
        }
    }

    Err(RtpError::Malformed(
        "no valid H.264 or H.265 SPS found in Annex-B stream".to_string(),
    ))
}
