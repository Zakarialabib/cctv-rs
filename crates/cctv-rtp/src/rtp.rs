//! RTP: the 12-byte header and its payload.
//!
//! Only what a camera feed needs is decoded — version, marker, payload type,
//! sequence, timestamp, SSRC — and the two optional tail pieces (CSRC list and
//! header extension) are skipped rather than exposed, because nothing downstream
//! uses them and a packet that carries them must still parse.

use crate::error::RtpError;

/// One RTP packet, borrowing its payload from the buffer it was parsed out of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RtpPacket<'a> {
    /// The marker bit.  For H.264 it flags the last packet of an access unit.
    pub marker: bool,
    /// The payload type, which is what the SDP `a=rtpmap` line names.
    pub payload_type: u8,
    /// The sequence number, for spotting loss.
    pub sequence: u16,
    /// The sampling timestamp; all packets of one frame share it.
    pub timestamp: u32,
    /// The synchronisation source.
    pub ssrc: u32,
    /// The payload, with padding and any extension already removed.
    pub payload: &'a [u8],
}

/// The fixed part of an RTP header.
const HEADER_LEN: usize = 12;

/// Parse one RTP packet.
///
/// # Errors
///
/// [`RtpError::Incomplete`] when the buffer is shorter than the header,
/// [`RtpError::Malformed`] when the version is not 2 or a declared CSRC list,
/// extension or padding runs past the end of the buffer.
pub fn parse_rtp(buf: &[u8]) -> Result<RtpPacket<'_>, RtpError> {
    if buf.len() < HEADER_LEN {
        return Err(RtpError::Incomplete);
    }

    let version = buf[0] >> 6;
    if version != 2 {
        return Err(RtpError::Malformed(format!(
            "RTP version {version}, expected 2"
        )));
    }
    let padding = buf[0] & 0x20 != 0;
    let extension = buf[0] & 0x10 != 0;
    let csrc_count = usize::from(buf[0] & 0x0F);

    let marker = buf[1] & 0x80 != 0;
    let payload_type = buf[1] & 0x7F;
    let sequence = u16::from_be_bytes([buf[2], buf[3]]);
    let timestamp = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let ssrc = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);

    let mut offset = HEADER_LEN
        .checked_add(csrc_count * 4)
        .ok_or_else(|| RtpError::Malformed("CSRC count overflows".into()))?;
    if buf.len() < offset {
        return Err(RtpError::Malformed("CSRC list runs past the packet".into()));
    }

    if extension {
        // 2 bytes profile + 2 bytes length in 32-bit words.
        let header = buf
            .get(offset..offset + 4)
            .ok_or_else(|| RtpError::Malformed("extension header is truncated".into()))?;
        let words = usize::from(u16::from_be_bytes([header[2], header[3]]));
        offset = offset
            .checked_add(4 + words * 4)
            .ok_or_else(|| RtpError::Malformed("extension length overflows".into()))?;
        if buf.len() < offset {
            return Err(RtpError::Malformed(
                "extension data runs past the packet".into(),
            ));
        }
    }

    let mut end = buf.len();
    if padding {
        // The last byte is the padding length, itself included.
        let last = *buf
            .last()
            .ok_or_else(|| RtpError::Malformed("padded packet is empty".into()))?;
        let strip = usize::from(last);
        if strip == 0 || strip > end - offset {
            return Err(RtpError::Malformed(format!(
                "padding length {strip} is out of range"
            )));
        }
        end -= strip;
    }

    Ok(RtpPacket {
        marker,
        payload_type,
        sequence,
        timestamp,
        ssrc,
        payload: &buf[offset..end],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal packet: version 2, marker set, PT 96, seq 1, ts 9000, ssrc 7.
    fn packet() -> Vec<u8> {
        let mut buf = vec![0x80, 0x80 | 96];
        buf.extend_from_slice(&1u16.to_be_bytes());
        buf.extend_from_slice(&9_000u32.to_be_bytes());
        buf.extend_from_slice(&7u32.to_be_bytes());
        buf.extend_from_slice(&[0x65, 0x88, 0x84]);
        buf
    }

    #[test]
    fn the_header_fields_are_read_and_the_payload_borrowed() {
        let buf = packet();
        let parsed = parse_rtp(&buf).expect("a packet");
        assert!(parsed.marker);
        assert_eq!(parsed.payload_type, 96);
        assert_eq!(parsed.sequence, 1);
        assert_eq!(parsed.timestamp, 9_000);
        assert_eq!(parsed.ssrc, 7);
        assert_eq!(parsed.payload, &[0x65, 0x88, 0x84]);
    }

    #[test]
    fn a_short_buffer_is_incomplete() {
        assert_eq!(parse_rtp(&[]), Err(RtpError::Incomplete));
        assert_eq!(parse_rtp(&[0x80; 11]), Err(RtpError::Incomplete));
    }

    #[test]
    fn a_header_only_packet_has_an_empty_payload() {
        let buf = packet();
        let parsed = parse_rtp(&buf[..HEADER_LEN]).expect("a packet");
        assert!(parsed.payload.is_empty());
    }

    #[test]
    fn a_wrong_version_is_refused() {
        let mut buf = packet();
        buf[0] = 0x40; // version 1
        assert!(matches!(parse_rtp(&buf), Err(RtpError::Malformed(_))));
    }

    #[test]
    fn padding_is_stripped() {
        let mut buf = packet();
        buf[0] |= 0x20; // P
        buf.extend_from_slice(&[0, 0, 0, 4]); // 4 bytes of padding, length 4
        let parsed = parse_rtp(&buf).expect("a packet");
        assert_eq!(parsed.payload, &[0x65, 0x88, 0x84]);
    }

    #[test]
    fn absurd_padding_is_malformed_rather_than_a_panic() {
        let mut buf = packet();
        buf[0] |= 0x20;
        buf.push(200); // claims 200 bytes of padding on a 16-byte packet
        assert!(matches!(parse_rtp(&buf), Err(RtpError::Malformed(_))));

        let mut zero = packet();
        zero[0] |= 0x20;
        zero.push(0); // padding length 0 is invalid
        assert!(matches!(parse_rtp(&zero), Err(RtpError::Malformed(_))));
    }

    #[test]
    fn a_csrc_list_and_an_extension_are_skipped() {
        let mut buf = vec![0x80 | 0x10 | 0x02, 96]; // X, CC=2
        buf.extend_from_slice(&1u16.to_be_bytes());
        buf.extend_from_slice(&9_000u32.to_be_bytes());
        buf.extend_from_slice(&7u32.to_be_bytes());
        buf.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 2]); // two CSRCs
        buf.extend_from_slice(&[0xBE, 0xDE, 0x00, 0x01]); // profile, 1 word
        buf.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // one extension word
        buf.extend_from_slice(&[0x65]);

        let parsed = parse_rtp(&buf).expect("a packet");
        assert_eq!(parsed.payload, &[0x65]);
    }

    #[test]
    fn a_truncated_extension_is_malformed() {
        let mut buf = vec![0x80 | 0x10, 96];
        buf.extend_from_slice(&[0; 10]);
        buf.extend_from_slice(&[0xBE, 0xDE, 0x00, 0x02]); // claims 2 words, none present
        assert!(matches!(parse_rtp(&buf), Err(RtpError::Malformed(_))));
    }

    #[test]
    fn a_truncated_csrc_list_is_malformed() {
        let mut buf = vec![0x80 | 0x04, 96]; // CC=4 needs 16 bytes of CSRC
        buf.extend_from_slice(&[0; 10]);
        assert!(matches!(parse_rtp(&buf), Err(RtpError::Malformed(_))));
    }
}
