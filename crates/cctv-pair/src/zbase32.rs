//! z-base-32 encoding and decoding implementation.
//!
//! Defined by Phil Zimmermann in:
//! <http://philzimmermann.com/docs/human-oriented-base-32-encoding.txt>
//!
//! Standard permutation:
//! `ybndrfg8ejkmcpqxot1uwisza345h769`

use std::fmt;

/// Permutation table for z-base-32.
pub const ZBASE32_ALPHABET: &[u8; 32] = b"ybndrfg8ejkmcpqxot1uwisza345h769";

/// Lookup table mapping ASCII byte to 5-bit integer, or 0xFF if invalid.
static ZBASE32_INV: [u8; 256] = {
    let mut table = [0xFFu8; 256];
    let mut i = 0usize;
    while i < 32 {
        table[ZBASE32_ALPHABET[i] as usize] = i as u8;
        i += 1;
    }
    table
};

/// Error decoding a z-base-32 string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZBase32Error {
    /// Invalid character encountered.
    InvalidCharacter(char),
    /// Truncated bits or invalid padding.
    InvalidLength,
}

impl fmt::Display for ZBase32Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCharacter(c) => write!(f, "invalid z-base-32 character: '{c}'"),
            Self::InvalidLength => write!(f, "invalid z-base-32 bit sequence / length"),
        }
    }
}

impl std::error::Error for ZBase32Error {}

/// Encodes binary data into a z-base-32 string.
pub fn encode(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }

    let mut out = String::with_capacity((data.len() * 8).div_ceil(5));
    let mut buffer: u64 = 0;
    let mut bits_in_buffer: u8 = 0;

    for &byte in data {
        buffer = (buffer << 8) | (byte as u64);
        bits_in_buffer += 8;
        while bits_in_buffer >= 5 {
            bits_in_buffer -= 5;
            let index = ((buffer >> bits_in_buffer) & 0x1F) as usize;
            out.push(ZBASE32_ALPHABET[index] as char);
        }
    }

    if bits_in_buffer > 0 {
        let index = ((buffer << (5 - bits_in_buffer)) & 0x1F) as usize;
        out.push(ZBASE32_ALPHABET[index] as char);
    }

    out
}

/// Decodes a z-base-32 string back to binary data.
pub fn decode(encoded: &str) -> Result<Vec<u8>, ZBase32Error> {
    if encoded.is_empty() {
        return Ok(Vec::new());
    }

    let mut out = Vec::with_capacity((encoded.len() * 5) / 8);
    let mut buffer: u64 = 0;
    let mut bits_in_buffer: u8 = 0;

    for c in encoded.chars() {
        let b = c as usize;
        if b >= 256 {
            return Err(ZBase32Error::InvalidCharacter(c));
        }
        let val = ZBASE32_INV[b];
        if val == 0xFF {
            return Err(ZBase32Error::InvalidCharacter(c));
        }
        buffer = (buffer << 5) | (val as u64);
        bits_in_buffer += 5;
        if bits_in_buffer >= 8 {
            bits_in_buffer -= 8;
            let byte = ((buffer >> bits_in_buffer) & 0xFF) as u8;
            out.push(byte);
        }
    }

    // Check that remaining unused bits in the buffer are 0
    if bits_in_buffer > 0 {
        let mask = (1 << bits_in_buffer) - 1;
        if (buffer & mask) != 0 {
            return Err(ZBase32Error::InvalidLength);
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zbase32_empty() {
        assert_eq!(encode(b""), "");
        assert_eq!(decode("").unwrap(), b"");
    }

    #[test]
    fn test_zbase32_roundtrip() {
        let vectors: &[&[u8]] = &[
            b"a",
            b"ab",
            b"abc",
            b"abcd",
            b"abcde",
            b"hello world",
            b"\x00\xff\x10\x88\x99\xaa\xbb\xcc",
            &[42u8; 32],
        ];

        for &vec in vectors {
            let enc = encode(vec);
            let dec = decode(&enc).expect("decode failed");
            assert_eq!(dec, vec);
        }
    }

    #[test]
    fn test_zbase32_invalid_chars() {
        assert!(matches!(
            decode("invalid_char_!"),
            Err(ZBase32Error::InvalidCharacter(_))
        ));
    }
}
