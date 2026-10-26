//! The RTSP codec's error type.

use std::fmt;

/// Why a byte slice could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RtspError {
    /// Not enough bytes yet.  This is **not** a failure: the caller reads more
    /// and tries again, which is what makes the parser safe on a socket.
    Incomplete,
    /// The bytes are there but they are not a valid message.
    Malformed(String),
    /// A header the codec needs is absent or unusable.
    MissingHeader(&'static str),
}

impl fmt::Display for RtspError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incomplete => f.write_str("incomplete message: read more bytes"),
            Self::Malformed(detail) => write!(f, "malformed message: {detail}"),
            Self::MissingHeader(name) => write!(f, "missing or unusable `{name}` header"),
        }
    }
}

impl std::error::Error for RtspError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_describe_themselves() {
        assert!(RtspError::Incomplete.to_string().contains("read more"));
        assert!(RtspError::Malformed("no CSeq".into())
            .to_string()
            .contains("no CSeq"));
        assert!(RtspError::MissingHeader("CSeq")
            .to_string()
            .contains("CSeq"));
    }
}
