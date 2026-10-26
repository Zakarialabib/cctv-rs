//! H.264 depacketisation (RFC 6184).
//!
//! A camera does not send frames; it sends RTP packets.  One frame may arrive as
//! a single NAL unit, as an aggregate of several (STAP-A), or split across many
//! packets (FU-A), and the only thing marking the end is the RTP marker bit.
//!
//! This module puts them back together.  Two failure modes matter and both are
//! handled explicitly rather than papered over:
//!
//! - a **lost packet** leaves a fragment that can never complete, so the whole
//!   access unit is dropped — a decoder handed half a frame produces garbage;
//! - a **timestamp change** mid-unit means the same thing, and is how the loss is
//!   usually noticed before the marker ever arrives.

use crate::types::AccessUnit;

/// Reassembles H.264 access units from RTP payloads.
///
/// Feed every payload of the video track with its marker bit and timestamp;
/// it returns an [`AccessUnit`] on the packet that ends one.
#[derive(Debug, Default)]
pub struct H264Depacketizer {
    timestamp: Option<u32>,
    nal_units: Vec<Vec<u8>>,
    fragment: Option<Vec<u8>>,
    dropped: u64,
}

impl H264Depacketizer {
    /// Create an empty depacketiser.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one RTP payload.
    ///
    /// Returns the completed access unit when `marker` ends one, otherwise
    /// `None`.  Payload types this crate does not reassemble (the interleaved
    /// modes 25–27, and the reserved ones) are ignored rather than corrupting
    /// the unit in progress.
    pub fn push(&mut self, payload: &[u8], marker: bool, timestamp: u32) -> Option<AccessUnit> {
        if payload.is_empty() {
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

        match payload[0] & 0x1F {
            // A whole NAL unit in one packet.
            1..=23 => self.nal_units.push(payload.to_vec()),
            // STAP-A: several NAL units, each length-prefixed.
            24 => self.push_stap_a(payload),
            // FU-A: one NAL unit across many packets.
            28 => self.push_fu_a(payload),
            // 0 and 30-31 are unspecified, 25-27 are the interleaved modes this
            // crate does not implement. Ignoring them keeps the unit intact.
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
        // An unterminated fragment means the last packet never arrived.
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

    /// Append every NAL unit in a STAP-A aggregate.
    fn push_stap_a(&mut self, payload: &[u8]) {
        let mut offset = 1;
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

    /// Append a FU-A fragment, rebuilding the NAL header on the first one.
    fn push_fu_a(&mut self, payload: &[u8]) {
        if payload.len() < 2 {
            return;
        }
        let fu_header = payload[1];
        let start = fu_header & 0x80 != 0;
        let end = fu_header & 0x40 != 0;
        let nal_type = fu_header & 0x1F;
        let data = &payload[2..];

        if start {
            // The real NAL header: forbidden bit and NRI from the FU indicator,
            // the unit type from the FU header.
            let header = (payload[0] & 0xE0) | nal_type;
            let mut nal = Vec::with_capacity(1 + data.len());
            nal.push(header);
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

    /// A NAL header for `nal_type` with NRI 3 and the forbidden bit clear.
    fn nal_header(nal_type: u8) -> u8 {
        0x60 | (nal_type & 0x1F)
    }

    #[test]
    fn a_single_packet_nal_is_an_access_unit_of_one() {
        let mut depacketizer = H264Depacketizer::new();
        let payload = [nal_header(5), 0x11, 0x22];
        let unit = depacketizer.push(&payload, true, 9_000).expect("a unit");
        assert_eq!(unit.timestamp, 9_000);
        assert_eq!(unit.nal_units, vec![payload.to_vec()]);
        assert_eq!(unit.bytes, 3);
        assert!(!depacketizer.in_progress());
        assert_eq!(depacketizer.dropped(), 0);
    }

    #[test]
    fn several_nal_units_accumulate_until_the_marker() {
        let mut depacketizer = H264Depacketizer::new();
        assert!(depacketizer
            .push(&[nal_header(7), 0x01], false, 5)
            .is_none());
        assert!(depacketizer.in_progress());
        assert!(depacketizer
            .push(&[nal_header(8), 0x02], false, 5)
            .is_none());
        let unit = depacketizer
            .push(&[nal_header(5), 0x03], true, 5)
            .expect("a unit");
        assert_eq!(unit.nal_units.len(), 3);
        assert_eq!(unit.bytes, 6);
    }

    #[test]
    fn a_fu_a_fragment_is_rebuilt_with_its_original_header() {
        let mut depacketizer = H264Depacketizer::new();
        // FU indicator: NRI 3, type 28.  FU header: start/end/type 5.
        let indicator = 0x60 | 28;

        assert!(depacketizer
            .push(&[indicator, 0x80 | 5, 0xAA, 0xBB], false, 7)
            .is_none());
        assert!(depacketizer
            .push(&[indicator, 0x05, 0xCC], false, 7)
            .is_none());
        let unit = depacketizer
            .push(&[indicator, 0x40 | 5, 0xDD], true, 7)
            .expect("a unit");

        // One NAL, header rebuilt as 0x65 (NRI 3, type 5), then the three chunks.
        assert_eq!(unit.nal_units.len(), 1);
        assert_eq!(unit.nal_units[0], vec![0x65, 0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn a_fu_a_that_is_one_packet_still_works() {
        let mut depacketizer = H264Depacketizer::new();
        let unit = depacketizer
            .push(&[0x60 | 28, 0x80 | 0x40 | 1, 0x42], true, 3)
            .expect("a unit");
        assert_eq!(unit.nal_units[0], vec![0x61, 0x42]);
    }

    #[test]
    fn a_stap_a_aggregate_yields_every_nal_it_carries() {
        let mut depacketizer = H264Depacketizer::new();
        // STAP-A header, then [len=2, SPS], [len=1, PPS].
        let payload = [0x60 | 24, 0x00, 0x02, 0x67, 0x42, 0x00, 0x01, 0x68];
        let unit = depacketizer.push(&payload, true, 11).expect("a unit");
        assert_eq!(unit.nal_units, vec![vec![0x67, 0x42], vec![0x68]]);
        assert_eq!(unit.bytes, 3);
    }

    #[test]
    fn a_truncated_stap_a_yields_no_unit_rather_than_a_partial_one() {
        let mut depacketizer = H264Depacketizer::new();
        // Claims 9 bytes of NAL, provides 1.
        let payload = [0x60 | 24, 0x00, 0x09, 0x67];
        assert!(
            depacketizer.push(&payload, true, 11).is_none(),
            "nothing whole was read, so there is no frame"
        );
        assert!(!depacketizer.in_progress());

        // An aggregate whose first NAL is whole and whose second is truncated
        // keeps the first: a decoder can still use a complete SPS.
        let mut depacketizer = H264Depacketizer::new();
        let payload = [0x60 | 24, 0x00, 0x02, 0x67, 0x42, 0x00, 0x09, 0x68];
        let unit = depacketizer.push(&payload, true, 12).expect("a unit");
        assert_eq!(unit.nal_units, vec![vec![0x67, 0x42]]);
    }

    #[test]
    fn a_lost_fragment_drops_the_whole_unit() {
        let mut depacketizer = H264Depacketizer::new();
        let indicator = 0x60 | 28;
        // Start, but no end ever arrives; the marker closes the unit.
        assert!(depacketizer
            .push(&[indicator, 0x80 | 5, 0xAA], false, 7)
            .is_none());
        assert!(depacketizer.push(&[], true, 7).is_none(), "no unit emitted");
        assert_eq!(depacketizer.dropped(), 1);
        assert!(!depacketizer.in_progress());
    }

    #[test]
    fn a_timestamp_change_mid_unit_drops_it() {
        let mut depacketizer = H264Depacketizer::new();
        assert!(depacketizer
            .push(&[nal_header(5), 0x01], false, 100)
            .is_none());
        // A new frame starts while the old one is unfinished: the packet between
        // them was lost, so the old unit can never be decoded.
        let unit = depacketizer
            .push(&[nal_header(5), 0x02], true, 200)
            .expect("a unit");
        assert_eq!(unit.timestamp, 200);
        assert_eq!(unit.nal_units, vec![vec![nal_header(5), 0x02]]);
        assert_eq!(depacketizer.dropped(), 1);
    }

    #[test]
    fn a_continuation_without_a_start_is_counted_and_ignored() {
        let mut depacketizer = H264Depacketizer::new();
        let indicator = 0x60 | 28;
        assert!(depacketizer
            .push(&[indicator, 0x05, 0xCC], false, 7)
            .is_none());
        assert_eq!(depacketizer.dropped(), 1);
        // The following unit is still fine.
        let unit = depacketizer
            .push(&[nal_header(5), 0x09], true, 7)
            .expect("a unit");
        assert_eq!(unit.nal_units.len(), 1);
    }

    #[test]
    fn an_empty_payload_with_a_marker_emits_nothing() {
        let mut depacketizer = H264Depacketizer::new();
        assert!(depacketizer.push(&[], true, 1).is_none());
        assert!(!depacketizer.in_progress());
        assert_eq!(depacketizer.dropped(), 0);
    }

    #[test]
    fn the_interleaved_and_reserved_types_do_not_corrupt_the_unit() {
        let mut depacketizer = H264Depacketizer::new();
        assert!(depacketizer
            .push(&[nal_header(5), 0x01], false, 1)
            .is_none());
        // Types 25-27 (STAP-B, MTAP, FU-B) and 30-31 are not reassembled here.
        for nal_type in [25u8, 26, 27, 30, 31, 0] {
            assert!(depacketizer
                .push(&[nal_header(nal_type), 0xFF], false, 1)
                .is_none());
        }
        let unit = depacketizer
            .push(&[nal_header(5), 0x02], true, 1)
            .expect("a unit");
        assert_eq!(
            unit.nal_units.len(),
            2,
            "only the two real NALs, nothing from the ignored types"
        );
    }

    #[test]
    fn consecutive_frames_are_independent() {
        let mut depacketizer = H264Depacketizer::new();
        let first = depacketizer
            .push(&[nal_header(5), 0x01], true, 10)
            .expect("a unit");
        let second = depacketizer
            .push(&[nal_header(5), 0x02], true, 20)
            .expect("a unit");
        assert_eq!(first.timestamp, 10);
        assert_eq!(second.timestamp, 20);
        assert_eq!(second.nal_units.len(), 1);
        assert_eq!(depacketizer.dropped(), 0);
    }
}
