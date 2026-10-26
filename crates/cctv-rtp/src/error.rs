//! The RTP payload's error type.
//!
//! Deliberately smaller than `cctv-rtsp`'s: a payload parser either has the bytes
//! or does not.

use std::fmt;

/// Why a payload could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RtpError {
    /// Not enough bytes yet.  **Not** a failure: the caller reads more and tries
    /// again, which is what makes the parser safe on a socket.
    Incomplete,
    /// The bytes are there but they are not a valid packet.
    Malformed(String),
}

impl fmt::Display for RtpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incomplete => f.write_str("incomplete RTP packet: read more bytes"),
            Self::Malformed(detail) => write!(f, "malformed RTP packet: {detail}"),
        }
    }
}

impl std::error::Error for RtpError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_describe_themselves() {
        assert!(RtpError::Incomplete.to_string().contains("read more"));
        assert!(RtpError::Malformed("bad".into())
            .to_string()
            .contains("bad"));
    }
}
