//! H.265 / HEVC depacketisation (RFC 7798).
//!
//! The HEVC twin of [`crate::h264`], and different in exactly two places that
//! matter: the NAL unit header is **two bytes** rather than one, and the
//! fragmentation unit carries a 6-bit type in a header *after* it.
//!
//! The unit types this crate reassembles:
//!
//! - `0..=47` — a whole NAL unit in one packet;
//! - `48` — AP, an aggregation packet of several units, each length-prefixed;
//! - `49` — FU, one unit split across many packets.
//!
//! `50` (PACI) and `51..=63` (reserved) are left alone rather than guessed at.

use crate::types::AccessUnit;

/// The two-byte HEVC NAL unit header, decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NalHeader {
    /// The NAL unit type (6 bits) — `32`/`33`/`34` are VPS/SPS/PPS, `19`/`20` are
    /// keyframes.
    pub nal_type: u8,
    /// The layer id (6 bits), always 0 for a camera feed.
    pub layer_id: u8,
    /// The temporal id (3 bits), as sent — the field stores `id + 1`.
    pub temporal_id: u8,
}

/// Decode a two-byte HEVC NAL unit header, or `None` when the slice is shorter.
pub fn parse_nal_header(buf: &[u8]) -> Option<NalHeader> {
    if buf.len() < 2 {
        return None;
    }
    Some(NalHeader {
        nal_type: (buf[0] >> 1) & 0x3F,
        layer_id: ((buf[0] & 0x01) << 5) | (buf[1] >> 3),
        temporal_id: buf[1] & 0x07,
    })
}

/// Reassembles HEVC access units from RTP payloads.
#[derive(Debug, Default)]
pub struct H265Depacketizer {
    timestamp: Option<u32>,
    nal_units: Vec<Vec<u8>>,
    fragment: Option<Vec<u8>>,
    dropped: u64,
}

impl H265Depacketizer {
    /// Create an empty depacketiser.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one RTP payload.
    ///
    /// Returns the completed access unit when `marker` ends one, otherwise
    /// `None`.  Payload types this crate does not reassemble are ignored rather
    /// than corrupting the unit in progress.
    pub fn push(&mut self, payload: &[u8], marker: bool, timestamp: u32) -> Option<AccessUnit> {
        if payload.len() < 2 {
            return self.finish(marker, timestamp);
        }

        if self.timestamp != Some(timestamp) {
            // A new timestamp with a unit still open means a packet was lost.
            if self.timestamp.is_some() && !self.nal_units.is_empty() {
                self.dropped += 1;
            }
            self.nal_units.clear();
            self.fragment = None;
            self.timestamp = Some(timestamp);
        }

        match (payload[0] >> 1) & 0x3F {
            0..=47 => self.nal_units.push(payload.to_vec()),
            48 => self.push_ap(payload),
            49 => self.push_fu(payload),
            // 50 (PACI) and 51-63 (reserved) are not reassembled here.
            _ => {}
        }

        self.finish(marker, timestamp)
    }

    /// How many access units were discarded because a packet went missing.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Whether a unit is part-way through.
    pub fn in_progress(&self) -> bool {
        self.timestamp.is_some()
    }

    /// Emit the unit when the marker says it is complete.
    fn finish(&mut self, marker: bool, timestamp: u32) -> Option<AccessUnit> {
        if !marker {
            return None;
        }
        if self.fragment.is_some() {
            self.fragment = None;
            self.nal_units.clear();
            self.timestamp = None;
            self.dropped += 1;
            return None;
        }
        if self.nal_units.is_empty() {
            self.timestamp = None;
            return None;
        }

        let nal_units = std::mem::take(&mut self.nal_units);
        self.timestamp = None;
        let bytes = nal_units.iter().map(Vec::len).sum();
        Some(AccessUnit {
            timestamp,
            nal_units,
            bytes,
        })
    }

    /// Append every NAL unit in an AP aggregate.
    fn push_ap(&mut self, payload: &[u8]) {
        let mut offset = 2;
        while offset + 2 <= payload.len() {
            let size = usize::from(u16::from_be_bytes([payload[offset], payload[offset + 1]]));
            offset += 2;
            if size == 0 || offset + size > payload.len() {
                // A truncated aggregate: keep what was read, stop guessing.
                return;
            }
            self.nal_units.push(payload[offset..offset + size].to_vec());
            offset += size;
        }
    }

    /// Append an FU fragment, rebuilding the two-byte NAL header on the first.
    fn push_fu(&mut self, payload: &[u8]) {
        if payload.len() < 3 {
            return;
        }
        let fu_header = payload[2];
        let start = fu_header & 0x80 != 0;
        let end = fu_header & 0x40 != 0;
        let fu_type = fu_header & 0x3F;
        let data = &payload[3..];

        if start {
            // Rebuild the header: keep the forbidden bit and the layer-id high
            // bit from the FU's own byte 0, take the type from the FU header.
            // Byte 1 (layer-id low + temporal id) is carried unchanged.
            let mut nal = Vec::with_capacity(2 + data.len());
            nal.push((payload[0] & 0x81) | (fu_type << 1));
            nal.push(payload[1]);
            nal.extend_from_slice(data);
            self.fragment = Some(nal);
        } else if let Some(fragment) = self.fragment.as_mut() {
            fragment.extend_from_slice(data);
        } else {
            // A continuation whose start was lost: nothing to attach it to.
            self.dropped += 1;
            return;
        }

        if end {
            if let Some(nal) = self.fragment.take() {
                self.nal_units.push(nal);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-byte HEVC NAL header for `nal_type`, layer 0, temporal id 1.
    fn header(nal_type: u8) -> [u8; 2] {
        [(nal_type & 0x3F) << 1, 0x01]
    }

    #[test]
    fn the_two_byte_header_decodes() {
        let parsed = parse_nal_header(&header(32)).expect("a header");
        assert_eq!(parsed.nal_type, 32, "VPS");
        assert_eq!(parsed.layer_id, 0);
        assert_eq!(parsed.temporal_id, 1);
        assert!(
            parse_nal_header(&[0x40]).is_none(),
            "one byte is not enough"
        );
    }

    #[test]
    fn a_single_packet_nal_is_an_access_unit_of_one() {
        let mut depacketizer = H265Depacketizer::new();
        let mut payload = header(19).to_vec(); // IDR_W_RADL, a keyframe
        payload.extend_from_slice(&[0xAA, 0xBB]);
        let unit = depacketizer.push(&payload, true, 9_000).expect("a unit");
        assert_eq!(unit.timestamp, 9_000);
        assert_eq!(unit.nal_units, vec![payload]);
        assert_eq!(unit.bytes, 4);
        assert!(!depacketizer.in_progress());
    }

    #[test]
    fn several_nal_units_accumulate_until_the_marker() {
        let mut depacketizer = H265Depacketizer::new();
        assert!(depacketizer.push(&header(32), false, 5).is_none());
        assert!(depacketizer.push(&header(33), false, 5).is_none());
        let unit = depacketizer.push(&header(34), true, 5).expect("a unit");
        assert_eq!(unit.nal_units.len(), 3, "VPS, SPS, PPS");
        assert_eq!(unit.bytes, 6);
    }

    #[test]
    fn an_ap_aggregate_yields_every_nal_it_carries() {
        let mut depacketizer = H265Depacketizer::new();
        // AP header, then [len=2, SPS], [len=2, PPS].
        let mut payload = header(48).to_vec();
        payload.extend_from_slice(&[0x00, 0x02]);
        payload.extend_from_slice(&header(33));
        payload.extend_from_slice(&[0x00, 0x02]);
        payload.extend_from_slice(&header(34));

        let unit = depacketizer.push(&payload, true, 11).expect("a unit");
        assert_eq!(
            unit.nal_units,
            vec![header(33).to_vec(), header(34).to_vec()]
        );
        assert_eq!(unit.bytes, 4);
    }

    #[test]
    fn an_fu_is_rebuilt_with_its_original_header() {
        let mut depacketizer = H265Depacketizer::new();
        // FU indicator: type 49, temporal id 1.
        let fu = header(49);

        // FU header: start, type 19 (IDR).
        assert!(depacketizer
            .push(&[fu[0], fu[1], 0x80 | 19, 0xAA, 0xBB], false, 7)
            .is_none());
        // Middle fragment.
        assert!(depacketizer
            .push(&[fu[0], fu[1], 19, 0xCC], false, 7)
            .is_none());
        // End fragment, and the marker.
        let unit = depacketizer
            .push(&[fu[0], fu[1], 0x40 | 19, 0xDD], true, 7)
            .expect("a unit");

        assert_eq!(unit.nal_units.len(), 1);
        let mut expected = header(19).to_vec();
        expected.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
        assert_eq!(unit.nal_units[0], expected);
        assert_eq!(depacketizer.dropped(), 0);
    }

    #[test]
    fn an_fu_that_is_one_packet_still_works() {
        let mut depacketizer = H265Depacketizer::new();
        let fu = header(49);
        let unit = depacketizer
            .push(&[fu[0], fu[1], 0x80 | 0x40 | 1, 0x42], true, 3)
            .expect("a unit");
        assert_eq!(unit.nal_units[0], vec![header(1)[0], header(1)[1], 0x42]);
    }

    #[test]
    fn a_lost_fragment_drops_the_whole_unit() {
        let mut depacketizer = H265Depacketizer::new();
        let fu = header(49);
        assert!(depacketizer
            .push(&[fu[0], fu[1], 0x80 | 19, 0xAA], false, 7)
            .is_none());
        // The end never arrives; the marker closes the unit anyway.
        assert!(depacketizer.push(&[], true, 7).is_none());
        assert_eq!(depacketizer.dropped(), 1);
        assert!(!depacketizer.in_progress());
    }

    #[test]
    fn a_timestamp_change_mid_unit_drops_it() {
        let mut depacketizer = H265Depacketizer::new();
        assert!(depacketizer.push(&header(1), false, 100).is_none());
        let unit = depacketizer.push(&header(1), true, 200).expect("a unit");
        assert_eq!(unit.timestamp, 200);
        assert_eq!(depacketizer.dropped(), 1);
    }

    #[test]
    fn a_continuation_without_a_start_is_counted_and_ignored() {
        let mut depacketizer = H265Depacketizer::new();
        let fu = header(49);
        assert!(depacketizer
            .push(&[fu[0], fu[1], 19, 0xCC], false, 7)
            .is_none());
        assert_eq!(depacketizer.dropped(), 1);
        let unit = depacketizer.push(&header(1), true, 7).expect("a unit");
        assert_eq!(unit.nal_units.len(), 1);
    }

    #[test]
    fn a_truncated_ap_yields_no_unit_rather_than_a_partial_one() {
        let mut depacketizer = H265Depacketizer::new();
        // Claims 9 bytes of NAL, provides 2.
        let mut payload = header(48).to_vec();
        payload.extend_from_slice(&[0x00, 0x09]);
        payload.extend_from_slice(&header(33));
        assert!(depacketizer.push(&payload, true, 11).is_none());
        assert!(!depacketizer.in_progress());

        // First NAL whole, second truncated: the whole one survives.
        let mut depacketizer = H265Depacketizer::new();
        let mut payload = header(48).to_vec();
        payload.extend_from_slice(&[0x00, 0x02]);
        payload.extend_from_slice(&header(33));
        payload.extend_from_slice(&[0x00, 0x09]);
        payload.extend_from_slice(&header(34));
        let unit = depacketizer.push(&payload, true, 12).expect("a unit");
        assert_eq!(unit.nal_units, vec![header(33).to_vec()]);
    }

    #[test]
    fn the_paci_and_reserved_types_do_not_corrupt_the_unit() {
        let mut depacketizer = H265Depacketizer::new();
        assert!(depacketizer.push(&header(1), false, 1).is_none());
        for nal_type in [50u8, 51, 63] {
            assert!(depacketizer.push(&header(nal_type), false, 1).is_none());
        }
        let unit = depacketizer.push(&header(1), true, 1).expect("a unit");
        assert_eq!(unit.nal_units.len(), 2, "only the two real NALs");
    }

    #[test]
    fn a_one_byte_payload_with_a_marker_emits_nothing() {
        let mut depacketizer = H265Depacketizer::new();
        assert!(depacketizer.push(&[0x40], true, 1).is_none());
        assert!(!depacketizer.in_progress());
        assert_eq!(depacketizer.dropped(), 0);
    }
}
