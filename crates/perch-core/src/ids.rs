//! Fixed-size byte values written as lowercase hex: author IDs, record IDs,
//! 32-byte hex fields, and signatures.
//!
//! Parsing accepts only lowercase hex of the exact length (ARCHITECTURE 4.1:
//! "uppercase hex is invalid"). Ordering of these types equals the ordering of
//! their hex strings, because hex encoding preserves byte order.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Error from parsing a lowercase hex string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HexError {
    /// The string has the wrong number of characters.
    #[error("expected {expected} lowercase hex characters, got {actual}")]
    Length {
        /// Required number of hex characters.
        expected: usize,
        /// Number of characters (bytes) in the input.
        actual: usize,
    },
    /// The string contains a character other than `0-9` or `a-f`.
    #[error("expected lowercase hex characters 0-9 and a-f only")]
    NotLowercaseHex,
}

/// Writes `bytes` as lowercase hex.
pub fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[(b >> 4) as usize] as char);
        s.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    s
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

/// Parses exactly `2 * N` lowercase hex characters into `N` bytes.
pub fn decode_hex<const N: usize>(s: &str) -> Result<[u8; N], HexError> {
    let b = s.as_bytes();
    if b.len() != 2 * N {
        return Err(HexError::Length { expected: 2 * N, actual: b.len() });
    }
    let mut out = [0u8; N];
    for (i, pair) in b.chunks_exact(2).enumerate() {
        let hi = nibble(pair[0]).ok_or(HexError::NotLowercaseHex)?;
        let lo = nibble(pair[1]).ok_or(HexError::NotLowercaseHex)?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

/// Returns true if `s` is exactly `len` lowercase hex characters.
pub fn is_lower_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|c| nibble(c).is_some())
}

macro_rules! hex_bytes_type {
    ($(#[$meta:meta])* $name:ident, $n:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub [u8; $n]);

        impl $name {
            /// Wraps raw bytes.
            pub const fn from_bytes(bytes: [u8; $n]) -> Self {
                Self(bytes)
            }
            /// The raw bytes.
            pub const fn as_bytes(&self) -> &[u8; $n] {
                &self.0
            }
            /// Lowercase hex form.
            pub fn to_hex(&self) -> String {
                encode_hex(&self.0)
            }
            /// Parses the lowercase hex form. Uppercase hex is rejected.
            pub fn from_hex(s: &str) -> Result<Self, HexError> {
                decode_hex::<$n>(s).map(Self)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.to_hex())
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.to_hex())
            }
        }

        impl FromStr for $name {
            type Err = HexError;
            fn from_str(s: &str) -> Result<Self, HexError> {
                Self::from_hex(s)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.to_hex())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = <std::borrow::Cow<'de, str>>::deserialize(d)?;
                Self::from_hex(&s).map_err(serde::de::Error::custom)
            }
        }
    };
}

hex_bytes_type!(
    /// An author: an Ed25519 public key (32 bytes, 64 lowercase hex characters).
    AuthorId,
    32
);
hex_bytes_type!(
    /// A record ID: SHA-256 of the canonical header (32 bytes, 64 lowercase hex).
    RecordId,
    32
);
hex_bytes_type!(
    /// Any other 32-byte value written as 64 lowercase hex characters
    /// (`Hex64` in ARCHITECTURE 4.5): `body_hash`, `person`, `nonce`, image `sha256`.
    Hex32,
    32
);
hex_bytes_type!(
    /// An Ed25519 signature (64 bytes, 128 lowercase hex characters).
    Sig,
    64
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip_and_strictness() {
        let id = RecordId([0xab; 32]);
        let h = id.to_hex();
        assert_eq!(h, "ab".repeat(32));
        assert_eq!(RecordId::from_hex(&h).unwrap(), id);
        assert_eq!(h.parse::<RecordId>().unwrap(), id);
        assert_eq!(
            RecordId::from_hex(&"AB".repeat(32)),
            Err(HexError::NotLowercaseHex)
        );
        assert_eq!(
            RecordId::from_hex("abc"),
            Err(HexError::Length { expected: 64, actual: 3 })
        );
        assert_eq!(RecordId::from_hex(&"g0".repeat(32)), Err(HexError::NotLowercaseHex));
        assert!(Sig::from_hex(&"00".repeat(64)).is_ok());
        assert!(Sig::from_hex(&"00".repeat(32)).is_err());
    }

    #[test]
    fn order_matches_hex_order() {
        let a = AuthorId([0x0f; 32]);
        let b = AuthorId([0xa0; 32]);
        assert!(a < b);
        assert!(a.to_hex() < b.to_hex());
    }

    #[test]
    fn serde_as_hex_string() {
        let id = AuthorId([0xab; 32]);
        let j = serde_json::to_string(&id).unwrap();
        assert_eq!(j, format!("\"{}\"", "ab".repeat(32)));
        let back: AuthorId = serde_json::from_str(&j).unwrap();
        assert_eq!(back, id);
        assert!(serde_json::from_str::<AuthorId>(&j.to_uppercase()).is_err());
    }
}
