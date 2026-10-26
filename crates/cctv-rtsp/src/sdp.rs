//! SDP: the session description `DESCRIBE` returns.
//!
//! What a camera session actually needs out of SDP is small and easy to get
//! wrong: the payload type that maps to the codec, the `a=control` attribute that
//! names the track's SETUP URL, and — for H.264 — the out-of-band SPS/PPS, which
//! a decoder needs before the first keyframe.

use base64::Engine as _;

/// One media track from a session description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// The media type (`video`, `audio`).
    pub media: String,
    /// The RTP payload type the track uses.
    pub payload_type: u8,
    /// The encoding name from `a=rtpmap` (`H264`), empty when there is none.
    pub encoding: String,
    /// The RTP clock rate from `a=rtpmap`, zero when there is none.
    pub clock_rate: u32,
    /// The `a=control` value: relative (`trackID=1`) or an absolute URL.
    pub control: String,
    /// H.264 `packetization-mode`. RFC 6184 defaults it to 0.
    pub packetization_mode: u8,
    /// Out-of-band parameter sets (SPS/PPS) from `sprop-parameter-sets`.
    pub parameter_sets: Vec<Vec<u8>>,
}

/// A parsed session description.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Session {
    /// The `s=` session name, empty when absent.
    pub name: String,
    /// The tracks, in file order.
    pub tracks: Vec<Track>,
}

impl Session {
    /// The first video track — the one a camera feed uses.
    pub fn video(&self) -> Option<&Track> {
        self.tracks.iter().find(|track| track.media == "video")
    }
}

/// Parse a session description.
///
/// Never fails: SDP is a flat list of attributes and a malformed line is skipped
/// rather than aborting the session, because a camera that emits one bad
/// attribute still streams.  A body with no `m=` line simply yields no tracks.
pub fn parse_sdp(body: &str) -> Session {
    let mut session = Session::default();
    let mut current: Option<Track> = None;

    for line in body.lines() {
        let line = line.trim_end_matches('\r');

        if let Some(rest) = line.strip_prefix("m=") {
            if let Some(track) = current.take() {
                session.tracks.push(track);
            }
            current = Some(media_section(rest));
            continue;
        }

        let Some(track) = current.as_mut() else {
            // Session-level attributes: only the name is used.
            if let Some(rest) = line.strip_prefix("s=") {
                session.name = rest.trim().to_string();
            }
            continue;
        };

        if let Some(rest) = line.strip_prefix("a=control:") {
            track.control = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("a=rtpmap:") {
            apply_rtpmap(track, rest);
        } else if let Some(rest) = line.strip_prefix("a=fmtp:") {
            apply_fmtp(track, rest);
        }
    }

    if let Some(track) = current {
        session.tracks.push(track);
    }
    session
}

/// Build a track from the body of an `m=` line.
fn media_section(rest: &str) -> Track {
    let mut parts = rest.split_whitespace();
    let media = parts.next().unwrap_or_default().to_string();
    // `m=video 0 RTP/AVP 96` — the payload type is the first one after the
    // transport profile; a track offering several lists them space-separated.
    let _port = parts.next();
    let _transport = parts.next();
    let payload_type = parts
        .next()
        .and_then(|token| token.parse::<u8>().ok())
        .unwrap_or(0);

    Track {
        media,
        payload_type,
        encoding: String::new(),
        clock_rate: 0,
        control: String::new(),
        packetization_mode: 0,
        parameter_sets: Vec::new(),
    }
}

/// Apply `a=rtpmap:<pt> <encoding>/<clock>[/<channels>]` when it names our PT.
fn apply_rtpmap(track: &mut Track, rest: &str) {
    let Some((pt, encoding)) = rest.trim().split_once(' ') else {
        return;
    };
    if pt.trim().parse::<u8>().ok() != Some(track.payload_type) {
        return;
    }
    let mut parts = encoding.trim().split('/');
    track.encoding = parts.next().unwrap_or_default().to_ascii_uppercase();
    track.clock_rate = parts
        .next()
        .and_then(|rate| rate.trim().parse().ok())
        .unwrap_or(0);
}

/// Apply `a=fmtp:<pt> key=value;key=value` when it names our PT.
fn apply_fmtp(track: &mut Track, rest: &str) {
    let Some((pt, params)) = rest.trim().split_once(' ') else {
        return;
    };
    if pt.trim().parse::<u8>().ok() != Some(track.payload_type) {
        return;
    }

    for pair in params.split(';') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key.trim() {
            "packetization-mode" => {
                if let Ok(mode) = value.trim().parse::<u8>() {
                    track.packetization_mode = mode;
                }
            }
            "sprop-parameter-sets" => {
                track.parameter_sets = value
                    .split(',')
                    .filter_map(|set| {
                        base64::engine::general_purpose::STANDARD
                            .decode(set.trim())
                            .ok()
                    })
                    .filter(|set| !set.is_empty())
                    .collect();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An Axis-shaped description: one H.264 video track with parameter sets.
    const AXIS: &str = "v=0\r\n\
o=- 1 1 IN IP4 192.168.1.64\r\n\
s=Media Presentation\r\n\
t=0 0\r\n\
a=control:*\r\n\
m=video 0 RTP/AVP 96\r\n\
a=control:trackID=1\r\n\
a=rtpmap:96 H264/90000\r\n\
a=fmtp:96 packetization-mode=1;profile-level-id=4d002a;sprop-parameter-sets=Z00AKpWoHgCJ+WbgICAoAAADAAEAAAMAMg8YMZYA,aO48gA==\r\n";

    #[test]
    fn a_video_track_is_read_field_by_field() {
        let session = parse_sdp(AXIS);
        assert_eq!(session.name, "Media Presentation");
        assert_eq!(session.tracks.len(), 1);

        let track = session.video().expect("a video track");
        assert_eq!(track.media, "video");
        assert_eq!(track.payload_type, 96);
        assert_eq!(track.encoding, "H264");
        assert_eq!(track.clock_rate, 90_000);
        assert_eq!(track.control, "trackID=1");
        assert_eq!(track.packetization_mode, 1);
        assert_eq!(track.parameter_sets.len(), 2, "SPS and PPS");
        // Both decode as base64; the SPS starts with the NAL header 0x67 (type 7).
        assert_eq!(track.parameter_sets[0][0] & 0x1F, 7, "SPS");
        assert_eq!(track.parameter_sets[1][0] & 0x1F, 8, "PPS");
    }

    #[test]
    fn a_track_without_rtpmap_or_fmtp_still_parses() {
        let session = parse_sdp("m=video 0 RTP/AVP 96\r\na=control:track1\r\n");
        let track = session.video().expect("a video track");
        assert_eq!(track.payload_type, 96);
        assert_eq!(track.encoding, "", "no rtpmap, no encoding name");
        assert_eq!(track.clock_rate, 0);
        assert_eq!(track.packetization_mode, 0, "RFC 6184 default");
        assert!(track.parameter_sets.is_empty());
    }

    #[test]
    fn an_rtpmap_for_another_payload_type_is_ignored() {
        // The camera offers 96 and 97; the m= line names 96, so 97's rtpmap must
        // not be applied to it.
        let body = "m=video 0 RTP/AVP 96\r\n\
                    a=rtpmap:97 MP4V-ES/90000\r\n\
                    a=rtpmap:96 H264/90000\r\n";
        let track = parse_sdp(body).video().cloned().expect("a track");
        assert_eq!(track.encoding, "H264");
        assert_eq!(track.clock_rate, 90_000);
    }

    #[test]
    fn several_tracks_are_kept_in_order_and_video_is_picked_out() {
        let body = "m=audio 0 RTP/AVP 8\r\n\
                    a=control:trackID=2\r\n\
                    a=rtpmap:8 PCMA/8000\r\n\
                    m=video 0 RTP/AVP 96\r\n\
                    a=control:trackID=1\r\n\
                    a=rtpmap:96 H264/90000\r\n";
        let session = parse_sdp(body);
        assert_eq!(session.tracks.len(), 2);
        assert_eq!(session.tracks[0].media, "audio");
        assert_eq!(session.tracks[0].encoding, "PCMA");
        assert_eq!(session.video().expect("video").control, "trackID=1");
    }

    #[test]
    fn a_description_without_media_has_no_tracks() {
        let session = parse_sdp("v=0\r\ns=Nothing\r\n");
        assert_eq!(session.name, "Nothing");
        assert!(session.tracks.is_empty());
        assert!(session.video().is_none());
        assert!(parse_sdp("").tracks.is_empty());
    }

    #[test]
    fn junk_lines_are_skipped_rather_than_aborting_the_session() {
        let body = "garbage\r\n\
                    m=video 0 RTP/AVP 96\r\n\
                    a=rtpmap:not-a-number H264/90000\r\n\
                    a=fmtp:96 packetization-mode=not-a-number\r\n\
                    a=control:trackID=1\r\n";
        let track = parse_sdp(body).video().cloned().expect("a track");
        assert_eq!(track.encoding, "", "the bad rtpmap was ignored");
        assert_eq!(track.packetization_mode, 0, "the bad mode kept the default");
        assert_eq!(track.control, "trackID=1");
    }

    #[test]
    fn an_unparseable_parameter_set_is_dropped_not_fatal() {
        let body = "m=video 0 RTP/AVP 96\r\n\
                    a=fmtp:96 sprop-parameter-sets=!!!not-base64!!!,aO48gA==\r\n";
        let track = parse_sdp(body).video().cloned().expect("a track");
        assert_eq!(track.parameter_sets.len(), 1, "only the valid one survives");
    }
}
