//! RTSP message framing.
//!
//! RTSP looks like HTTP/1.1 with a different method set and a different start
//! line, and over TCP it multiplexes control messages with binary media on the
//! *same* connection: a media frame is `$`, one byte of channel, a two-byte
//! big-endian length, then the payload.  Both forms are parsed here, and the
//! parser is incremental — [`Message::parse`] returns
//! [`RtspError::Incomplete`] when it needs more bytes rather than guessing.

use std::fmt;

use crate::error::RtspError;

/// The RTSP methods a camera session needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// `OPTIONS` — capabilities, and the keepalive.
    Options,
    /// `DESCRIBE` — fetch the session description.
    Describe,
    /// `SETUP` — bind one track to a transport.
    Setup,
    /// `PLAY` — start (or resume) delivery.
    Play,
    /// `PAUSE` — stop delivery without tearing the session down.
    Pause,
    /// `TEARDOWN` — end the session.
    Teardown,
}

impl Method {
    /// The method token as sent on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Options => "OPTIONS",
            Self::Describe => "DESCRIBE",
            Self::Setup => "SETUP",
            Self::Play => "PLAY",
            Self::Pause => "PAUSE",
            Self::Teardown => "TEARDOWN",
        }
    }

    /// Parse a method token, or `None` when it is not one this crate speaks.
    pub fn parse(token: &str) -> Option<Self> {
        match token.trim() {
            "OPTIONS" => Some(Self::Options),
            "DESCRIBE" => Some(Self::Describe),
            "SETUP" => Some(Self::Setup),
            "PLAY" => Some(Self::Play),
            "PAUSE" => Some(Self::Pause),
            "TEARDOWN" => Some(Self::Teardown),
            _ => None,
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Message headers, kept in wire order.
///
/// Lookup is case-insensitive: RTSP header names are not case-sensitive, and a
/// camera that sends `cseq` is not wrong.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headers {
    entries: Vec<(String, String)>,
}

impl Headers {
    /// The value of a header, matched case-insensitively.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The `CSeq` header as a number.
    pub fn cseq(&self) -> Option<u32> {
        self.get("CSeq").and_then(|value| value.trim().parse().ok())
    }

    /// The `Content-Length` header, defaulting to zero as RTSP requires.
    pub fn content_length(&self) -> Result<usize, RtspError> {
        match self.get("Content-Length") {
            None => Ok(0),
            Some(value) => value.trim().parse().map_err(|_| {
                RtspError::Malformed(format!("Content-Length is not a number: {value:?}"))
            }),
        }
    }

    /// Add a header.
    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.entries.push((name.into(), value.into()));
    }

    /// Every header, in wire order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    /// How many headers there are.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no headers.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// One RTSP request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The method.
    pub method: Method,
    /// The request URI — a full `rtsp://` URL, or the session URI.
    pub uri: String,
    /// The headers, `CSeq` included.
    pub headers: Headers,
    /// The body (SDP for `DESCRIBE` is in the *response*, so usually empty).
    pub body: Vec<u8>,
}

impl Request {
    /// Build a request with the `CSeq` and `User-Agent` headers filled in.
    pub fn new(method: Method, uri: impl Into<String>, cseq: u32) -> Self {
        let mut headers = Headers::default();
        headers.insert("CSeq", cseq.to_string());
        headers.insert("User-Agent", "cctv-rs");
        Self {
            method,
            uri: uri.into(),
            headers,
            body: Vec::new(),
        }
    }

    /// Serialise to bytes, with `Content-Length` added when there is a body.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = format!("{} {} RTSP/1.0\r\n", self.method.as_str(), self.uri);
        for (name, value) in self.headers.iter() {
            out.push_str(name);
            out.push_str(": ");
            out.push_str(value);
            out.push_str("\r\n");
        }
        if !self.body.is_empty() {
            out.push_str(&format!("Content-Length: {}\r\n", self.body.len()));
        }
        out.push_str("\r\n");
        let mut bytes = out.into_bytes();
        bytes.extend_from_slice(&self.body);
        bytes
    }
}

/// One RTSP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// The status code.
    pub status: u16,
    /// The reason phrase, as sent (it is informational only).
    pub reason: String,
    /// The headers.
    pub headers: Headers,
    /// The body, when `Content-Length` said there was one.
    pub body: Vec<u8>,
}

impl Response {
    /// Whether the status is `2xx`.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// A decoded RTSP message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// A request (a camera does not send these, but a test server does).
    Request(Request),
    /// A response.
    Response(Response),
}

impl Message {
    /// Decode one message from the front of `buf`.
    ///
    /// Returns the message and how many bytes it consumed, so the caller can
    /// keep the remainder for the next call.  [`RtspError::Incomplete`] means the
    /// buffer does not hold a whole message yet — read more and retry.
    pub fn parse(buf: &[u8]) -> Result<(Self, usize), RtspError> {
        let Some(head_end) = find_header_end(buf) else {
            // A cap keeps a hostile peer from growing the buffer forever.
            if buf.len() > MAX_HEADER_BYTES {
                return Err(RtspError::Malformed(
                    "headers exceed 64 KiB without terminating".into(),
                ));
            }
            return Err(RtspError::Incomplete);
        };

        let head = std::str::from_utf8(&buf[..head_end])
            .map_err(|_| RtspError::Malformed("headers are not UTF-8".into()))?;
        let mut lines = head.split("\r\n");
        let start = lines
            .next()
            .ok_or_else(|| RtspError::Malformed("empty message".into()))?;

        let mut headers = Headers::default();
        for line in lines {
            if line.is_empty() {
                continue;
            }
            let (name, value) = line
                .split_once(':')
                .ok_or_else(|| RtspError::Malformed(format!("header without a colon: {line:?}")))?;
            headers.insert(name.trim(), value.trim());
        }

        let body_start = head_end + 4;
        let length = headers.content_length()?;
        let total = body_start
            .checked_add(length)
            .ok_or_else(|| RtspError::Malformed("Content-Length overflows".into()))?;
        if buf.len() < total {
            return Err(RtspError::Incomplete);
        }
        let body = buf[body_start..total].to_vec();

        let message = if let Some(rest) = start.strip_prefix("RTSP/") {
            let (_version, rest) = rest
                .split_once(' ')
                .ok_or_else(|| RtspError::Malformed(format!("bad status line: {start:?}")))?;
            let (code, reason) = rest.split_once(' ').unwrap_or((rest, ""));
            let status = code
                .trim()
                .parse::<u16>()
                .map_err(|_| RtspError::Malformed(format!("status is not a number: {code:?}")))?;
            Self::Response(Response {
                status,
                reason: reason.trim().to_string(),
                headers,
                body,
            })
        } else {
            let mut parts = start.splitn(3, ' ');
            let method_token = parts
                .next()
                .ok_or_else(|| RtspError::Malformed("empty request line".into()))?;
            let method = Method::parse(method_token).ok_or_else(|| {
                RtspError::Malformed(format!("unsupported method {method_token:?}"))
            })?;
            let uri = parts
                .next()
                .ok_or_else(|| RtspError::Malformed(format!("no URI in {start:?}")))?
                .to_string();
            Self::Request(Request {
                method,
                uri,
                headers,
                body,
            })
        };

        Ok((message, total))
    }
}

/// One interleaved media frame: a channel and its payload.
///
/// Over TCP the media shares the control connection, framed as
/// `$ <channel> <length hi> <length lo> <payload>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interleaved<'a> {
    /// The channel the server assigned in `SETUP`.
    pub channel: u8,
    /// The RTP packet.
    pub payload: &'a [u8],
}

/// Decode one interleaved frame from the front of `buf`.
///
/// `None` when the buffer does not start with `$` or does not hold the whole
/// frame yet.
pub fn parse_interleaved(buf: &[u8]) -> Option<Result<(Interleaved<'_>, usize), RtspError>> {
    if buf.first() != Some(&b'$') {
        return None;
    }
    if buf.len() < 4 {
        return None;
    }
    let channel = buf[1];
    let length = usize::from(u16::from_be_bytes([buf[2], buf[3]]));
    let end = 4usize.checked_add(length)?;
    if buf.len() < end {
        return None;
    }
    Some(Ok((
        Interleaved {
            channel,
            payload: &buf[4..end],
        },
        end,
    )))
}

/// The largest header block this parser will buffer before giving up.
const MAX_HEADER_BYTES: usize = 64 * 1024;

/// The index of the `\r\n\r\n` that ends the start line and headers.
fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESCRIBE_RESPONSE: &str = "RTSP/1.0 200 OK\r\n\
         CSeq: 2\r\n\
         Content-Type: application/sdp\r\n\
         Content-Length: 9\r\n\
         \r\n\
         v=0\r\nm=vi";

    #[test]
    fn a_response_is_read_field_by_field() {
        let (message, used) = Message::parse(DESCRIBE_RESPONSE.as_bytes()).expect("a response");
        assert_eq!(used, DESCRIBE_RESPONSE.len());
        let Message::Response(response) = message else {
            panic!("not a response");
        };
        assert_eq!(response.status, 200);
        assert_eq!(response.reason, "OK");
        assert_eq!(response.headers.cseq(), Some(2));
        assert_eq!(
            response.headers.get("content-type"),
            Some("application/sdp")
        );
        assert_eq!(response.body, b"v=0\r\nm=vi");
        assert!(response.is_success());
    }

    #[test]
    fn a_partial_message_is_incomplete_not_malformed() {
        // Everything but the body: the parser must ask for more, not guess.
        let partial = &DESCRIBE_RESPONSE.as_bytes()[..DESCRIBE_RESPONSE.len() - 3];
        assert_eq!(Message::parse(partial), Err(RtspError::Incomplete));
        // And a truncated header block, too.
        assert_eq!(
            Message::parse(b"RTSP/1.0 200 OK\r\nCSeq:"),
            Err(RtspError::Incomplete)
        );
        assert_eq!(Message::parse(b""), Err(RtspError::Incomplete));
    }

    #[test]
    fn the_remainder_of_the_buffer_is_left_for_the_next_message() {
        let mut buf = DESCRIBE_RESPONSE.as_bytes().to_vec();
        buf.extend_from_slice(b"$data");
        let (_, used) = Message::parse(&buf).expect("a response");
        assert_eq!(&buf[used..], b"$data");
    }

    #[test]
    fn a_request_round_trips_through_its_own_serialiser() {
        let mut request = Request::new(Method::Setup, "rtsp://cam/stream/track1", 7);
        request
            .headers
            .insert("Transport", "RTP/AVP/TCP;interleaved=0-1");
        let bytes = request.to_bytes();
        assert!(bytes.starts_with(b"SETUP rtsp://cam/stream/track1 RTSP/1.0\r\n"));

        let (message, used) = Message::parse(&bytes).expect("a request");
        assert_eq!(used, bytes.len());
        let Message::Request(parsed) = message else {
            panic!("not a request");
        };
        assert_eq!(parsed.method, Method::Setup);
        assert_eq!(parsed.uri, "rtsp://cam/stream/track1");
        assert_eq!(parsed.headers.cseq(), Some(7));
        assert_eq!(
            parsed.headers.get("transport"),
            Some("RTP/AVP/TCP;interleaved=0-1")
        );
        assert!(parsed.body.is_empty());
        // No body, so no Content-Length is emitted.
        assert!(!bytes.windows(14).any(|w| w == b"Content-Length"));
    }

    #[test]
    fn a_request_body_is_announced_with_its_length() {
        let mut request = Request::new(Method::Play, "rtsp://cam/s", 9);
        request.body = b"hello".to_vec();
        let bytes = request.to_bytes();
        assert!(bytes
            .windows(19)
            .any(|window| window == b"Content-Length: 5\r\n"));
        let (message, _) = Message::parse(&bytes).expect("a request");
        let Message::Request(parsed) = message else {
            panic!("not a request");
        };
        assert_eq!(parsed.body, b"hello");
    }

    #[test]
    fn a_non_success_status_is_not_success() {
        let bytes = b"RTSP/1.0 401 Unauthorized\r\nCSeq: 1\r\nWWW-Authenticate: Digest realm=\"cam\"\r\n\r\n";
        let (message, _) = Message::parse(bytes).expect("a response");
        let Message::Response(response) = message else {
            panic!("not a response");
        };
        assert_eq!(response.status, 401);
        assert!(!response.is_success());
        assert_eq!(
            response.headers.get("WWW-Authenticate"),
            Some("Digest realm=\"cam\"")
        );
    }

    #[test]
    fn malformed_input_is_reported_rather_than_panicking() {
        let no_colon = b"RTSP/1.0 200 OK\r\nCSeq 1\r\n\r\n";
        assert!(matches!(
            Message::parse(no_colon),
            Err(RtspError::Malformed(_))
        ));

        let bad_status = b"RTSP/1.0 abc OK\r\n\r\n";
        assert!(matches!(
            Message::parse(bad_status),
            Err(RtspError::Malformed(_))
        ));

        let bad_length = b"RTSP/1.0 200 OK\r\nContent-Length: many\r\n\r\n";
        assert!(matches!(
            Message::parse(bad_length),
            Err(RtspError::Malformed(_))
        ));

        let bad_method = b"FLY rtsp://cam/ RTSP/1.0\r\n\r\n";
        assert!(matches!(
            Message::parse(bad_method),
            Err(RtspError::Malformed(_))
        ));
    }

    #[test]
    fn an_interleaved_frame_carries_its_channel_and_payload() {
        let buf = [b'$', 2, 0, 3, 0xAA, 0xBB, 0xCC];
        let (frame, used) = parse_interleaved(&buf)
            .expect("an interleaved frame")
            .expect("valid");
        assert_eq!(frame.channel, 2);
        assert_eq!(frame.payload, &[0xAA, 0xBB, 0xCC]);
        assert_eq!(used, buf.len());
    }

    #[test]
    fn a_truncated_or_absent_interleaved_frame_yields_none() {
        // Not interleaved at all.
        assert!(parse_interleaved(b"RTSP/1.0").is_none());
        // Header cut short.
        assert!(parse_interleaved(&[b'$', 0]).is_none());
        // Payload cut short: 3 announced, 2 present.
        assert!(parse_interleaved(&[b'$', 0, 0, 3, 0xAA, 0xBB]).is_none());
    }

    #[test]
    fn headers_are_case_insensitive_and_keep_wire_order() {
        let mut headers = Headers::default();
        headers.insert("cseq", "4");
        headers.insert("Transport", "RTP/AVP/TCP");
        assert_eq!(headers.cseq(), Some(4));
        assert_eq!(headers.get("CSEQ"), Some("4"));
        assert_eq!(headers.get("Transport"), Some("RTP/AVP/TCP"));
        assert_eq!(headers.len(), 2);
        assert!(!headers.is_empty());
        let names: Vec<&str> = headers.iter().map(|(name, _)| name).collect();
        assert_eq!(names, vec!["cseq", "Transport"]);
    }

    #[test]
    fn every_method_round_trips() {
        for method in [
            Method::Options,
            Method::Describe,
            Method::Setup,
            Method::Play,
            Method::Pause,
            Method::Teardown,
        ] {
            assert_eq!(Method::parse(method.as_str()), Some(method));
            assert_eq!(method.to_string(), method.as_str());
        }
        assert_eq!(Method::parse("RECORD"), None);
    }
}
