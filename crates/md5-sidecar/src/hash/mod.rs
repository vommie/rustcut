//! What a sidecar holds: an MD5 as 32 lowercase hex digits (rules 2 and 3).

use std::fmt;
use std::str::FromStr;

/// The longest sidecar content that is read (rule 3); a longer one is invalid.
pub const MAX_LEN: usize = 4096;

/// The MD5 of zero bytes. A zero-byte video may have a sidecar with it, but never counts as known (rule 8).
pub const ZERO_BYTE_MD5: &str = "d41d8cd98f00b204e9800998ecf8427e";

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// An MD5 as 32 lowercase hex digits, the form RustView stores and every sidecar is written in.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Md5Hex([u8; 32]);

impl Md5Hex {
    /// From the 16 bytes of a finished digest.
    pub fn from_digest(digest: [u8; 16]) -> Self {
        let mut out = [0_u8; 32];
        let digits = digest
            .iter()
            .flat_map(|byte| [hex_digit(byte >> 4), hex_digit(byte & 0x0f)]);
        for (slot, digit) in out.iter_mut().zip(digits) {
            *slot = digit;
        }
        Self(out)
    }

    /// Exactly 32 hex digits in any case (lowercased); anything else is `None`.
    pub fn from_hex(token: &[u8]) -> Option<Self> {
        let digits: &[u8; 32] = token.try_into().ok()?;
        let mut out = [0_u8; 32];
        for (slot, &digit) in out.iter_mut().zip(digits) {
            if !digit.is_ascii_hexdigit() {
                return None;
            }
            *slot = digit.to_ascii_lowercase();
        }
        Some(Self(out))
    }

    /// The 32 lowercase hex digits.
    pub fn as_str(&self) -> &str {
        // Only ASCII hex digits are ever stored, so this never falls back.
        std::str::from_utf8(&self.0).unwrap_or_default()
    }

    /// The 32 bytes a sidecar is written with.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Whether this is the MD5 of zero bytes, which never counts as known (rule 8).
    pub fn is_zero_byte(&self) -> bool {
        self.as_str() == ZERO_BYTE_MD5
    }
}

fn hex_digit(nibble: u8) -> u8 {
    match nibble & 0x0f {
        low @ 0..=9 => b'0' + low,
        high => b'a' + (high - 10),
    }
}

impl fmt::Display for Md5Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for Md5Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Md5Hex({})", self.as_str())
    }
}

impl FromStr for Md5Hex {
    type Err = Invalid;

    /// Exactly 32 hex digits, nothing around them (use [`parse`] for a sidecar's content).
    fn from_str(s: &str) -> Result<Self, Invalid> {
        Self::from_hex(s.as_bytes()).ok_or(Invalid::NotAHash)
    }
}

/// Why a sidecar does not count: it is treated as missing (rule 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invalid {
    /// Nothing but white space (or a byte order mark).
    Empty,
    /// More than [`MAX_LEN`] bytes.
    TooLong,
    /// The first token is not 32 hex digits.
    NotAHash,
    /// A folder, a symbolic link or anything else that is not a regular file.
    NotAFile,
    /// It could not be read (the reason is logged).
    Unreadable,
}

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "it is empty",
            Self::TooLong => "it is longer than 4096 bytes",
            Self::NotAHash => "it does not start with 32 hex digits",
            Self::NotAFile => "it is not a regular file",
            Self::Unreadable => "it cannot be read",
        })
    }
}

impl std::error::Error for Invalid {}

/// Reads a sidecar's content (rule 3): at most [`MAX_LEN`] bytes, a UTF-8 byte order mark and one leading backslash
/// (md5sum's mark for an escaped name) are skipped, and the first white-space-separated token must be 32 hex digits
/// in any case. So the bare hash and md5sum's `<hash>  <name>` line both read, and the result is lowercase.
pub fn parse(content: &[u8]) -> Result<Md5Hex, Invalid> {
    if content.len() > MAX_LEN {
        return Err(Invalid::TooLong);
    }
    let rest = content.strip_prefix(UTF8_BOM).unwrap_or(content);
    let rest = rest.strip_prefix(b"\\").unwrap_or(rest);
    let token = rest
        .split(u8::is_ascii_whitespace)
        .find(|token| !token.is_empty())
        .ok_or(Invalid::Empty)?;
    Md5Hex::from_hex(token).ok_or(Invalid::NotAHash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_becomes_lowercase_hex() {
        let digest = [
            0xd4, 0x1d, 0x8c, 0xd9, 0x8f, 0x00, 0xb2, 0x04, 0xe9, 0x80, 0x09, 0x98, 0xec, 0xf8, 0x42, 0x7e,
        ];
        let hash = Md5Hex::from_digest(digest);
        assert_eq!(hash.as_str(), ZERO_BYTE_MD5);
        assert!(hash.is_zero_byte());
        assert_eq!(Md5Hex::from_digest([0xff; 16]).as_str(), "f".repeat(32));
    }

    #[test]
    fn from_str_wants_exactly_the_digits() {
        assert!(
            "D41D8CD98F00B204E9800998ECF8427E"
                .parse::<Md5Hex>()
                .is_ok_and(|h| h.is_zero_byte())
        );
        assert_eq!(
            " d41d8cd98f00b204e9800998ecf8427e".parse::<Md5Hex>(),
            Err(Invalid::NotAHash)
        );
    }
}
