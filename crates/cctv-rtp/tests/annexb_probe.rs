//! Integration tests for Annex-B stream probing and MediaDescriptor verification.

use cctv_rtp::annexb::VideoCodec;
use cctv_rtp::probe::{MediaDescriptor, MediaProbe, TranscodePlan};

#[test]
fn test_h264_1080p_annexb_matches_mp4_avcc() {
    // 1080p H.264 SPS: Profile High (100), Level 4.1 (41), 1920x1080
    let sps_nal = [103, 100, 0, 41, 172, 236, 7, 128, 34, 126, 88];

    // Build Annex-B stream: [start_code_4, SPS, start_code_3, PPS]
    let mut annexb_stream = Vec::new();
    annexb_stream.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
    annexb_stream.extend_from_slice(&sps_nal);
    annexb_stream.extend_from_slice(&[0x00, 0x00, 0x01, 0x68, 0xce, 0x3c, 0x80]); // PPS

    let stream_descriptor =
        MediaProbe::probe_stream(&annexb_stream).expect("Annex-B probe succeeds");

    assert_eq!(
        stream_descriptor,
        MediaDescriptor {
            codec: VideoCodec::H264,
            profile: 100,
            level: 41,
            width: 1920,
            height: 1080,
        }
    );

    // Build mock MP4 file containing an avcC box with the exact same SPS
    let mut mp4_bytes = Vec::new();
    mp4_bytes.extend_from_slice(b"\x00\x00\x00\x20ftypisom\x00\x00\x02\x00isomiso2avc1mp41");
    // avcC box
    let sps_len = sps_nal.len() as u16;
    let box_len = (4 + 6 + 2 + sps_nal.len()) as u32;
    mp4_bytes.extend_from_slice(&box_len.to_be_bytes());
    mp4_bytes.extend_from_slice(b"avcC");
    mp4_bytes.push(1); // config version
    mp4_bytes.push(100); // profile
    mp4_bytes.push(0); // profile compat
    mp4_bytes.push(41); // level
    mp4_bytes.push(0xFF); // lengthSizeMinusOne
    mp4_bytes.push(0xE1); // 1 SPS
    mp4_bytes.extend_from_slice(&sps_len.to_be_bytes());
    mp4_bytes.extend_from_slice(&sps_nal);

    let mp4_descriptor = MediaProbe::probe_bytes(&mp4_bytes).expect("MP4 avcC probe succeeds");

    // Exit criterion 1: Test: a 1080p H.264 Annex-B stream produces the same descriptor as an MP4 containing the same stream.
    assert_eq!(stream_descriptor, mp4_descriptor);

    let plan = TranscodePlan::for_descriptor(stream_descriptor);
    assert!(plan.passthrough_possible);
    assert_eq!(plan.target_width, 1920);
    assert_eq!(plan.target_height, 1080);
}

#[test]
fn test_4k_hevc_annexb_profile_and_level() {
    // 4K (3840x2160) HEVC SPS: Profile Main (1), Level 5.1 (153)
    let sps_nal = [
        66, 1, 1, 1, 64, 0, 0, 0, 176, 0, 0, 0, 0, 0, 153, 160, 1, 224, 32, 2, 28, 80,
    ];

    // Build Annex-B stream
    let mut annexb_stream = Vec::new();
    // VPS
    annexb_stream.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x40, 0x01, 0x0c, 0x01]);
    // SPS
    annexb_stream.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
    annexb_stream.extend_from_slice(&sps_nal);
    // PPS
    annexb_stream.extend_from_slice(&[0x00, 0x00, 0x01, 0x44, 0x01, 0xc0]);

    // Exit criterion 2: Test: a 4K HEVC Annex-B stream produces the correct profile/level.
    let descriptor = MediaProbe::probe_stream(&annexb_stream).expect("HEVC stream probe succeeds");
    assert_eq!(descriptor.codec, VideoCodec::H265);
    assert_eq!(descriptor.profile, 1);
    assert_eq!(descriptor.level, 153);
    assert_eq!(descriptor.width, 3840);
    assert_eq!(descriptor.height, 2160);

    let plan = TranscodePlan::for_descriptor(descriptor);
    assert!(!plan.passthrough_possible);
    assert_eq!(plan.target_width, 1920);
    assert_eq!(plan.target_height, 1080);
}

#[test]
fn test_emulation_prevention_unescaping() {
    let raw = [0x00, 0x00, 0x03, 0x01, 0x00, 0x00, 0x03, 0x02];
    let unescaped = cctv_rtp::unescape_rbsp(&raw);
    assert_eq!(unescaped, vec![0x00, 0x00, 0x01, 0x00, 0x00, 0x02]);
}

#[test]
fn test_incomplete_and_malformed_streams() {
    assert!(MediaProbe::probe_stream(&[]).is_err());
    assert!(MediaProbe::probe_stream(&[0x00, 0x00, 0x01, 0x05]).is_err()); // Only slice NAL
    assert!(MediaProbe::probe_bytes(&[]).is_err());
}
