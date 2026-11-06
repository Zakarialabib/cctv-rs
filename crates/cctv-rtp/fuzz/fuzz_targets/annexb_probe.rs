#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // 1. Fuzz probe_annexb directly (Annex-B stream splitting, unescape RBSP, SPS parsing)
    let _ = cctv_rtp::annexb::probe_annexb(data);

    // 2. Fuzz high-level probe_stream and probe_bytes on MediaProbe
    let _ = cctv_rtp::MediaProbe::probe_stream(data);
    let _ = cctv_rtp::MediaProbe::probe_bytes(data);

    // 3. Fuzz split_annexb and individual NAL unescaping & parsing
    let nals = cctv_rtp::annexb::split_annexb(data);
    for nal in nals {
        let _ = cctv_rtp::annexb::parse_h264_sps(nal);
        let _ = cctv_rtp::annexb::parse_h265_sps(nal);
        let _ = cctv_rtp::annexb::unescape_rbsp(nal);
    }

    // 4. Directly feed raw byte slice to SPS parsers (as untrusted NALs)
    let _ = cctv_rtp::annexb::parse_h264_sps(data);
    let _ = cctv_rtp::annexb::parse_h265_sps(data);

    // 5. Test BitReader exp-Golomb decode on arbitrary byte sequences
    let mut reader = cctv_rtp::annexb::BitReader::new(data);
    while reader.remaining_bits() > 0 {
        if reader.read_ue().is_none() {
            break;
        }
        if reader.read_se().is_none() {
            break;
        }
        if reader.read_bits(3).is_none() {
            break;
        }
    }
});
