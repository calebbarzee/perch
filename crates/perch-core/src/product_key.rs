//! Product keys (ARCHITECTURE 4.8) and GTIN validation.
//!
//! A [`ProductKey`] is a string in canonical form. Records group by exact
//! string equality, so a key field (`listing.product`, `review.product`,
//! `observation.product`) must already be canonical:
//!
//! | Form | Rule |
//! |---|---|
//! | `gtin:` + 14 digits | Valid GS1 check digit. Shorter GTINs are zero-padded to 14 before use. |
//! | `mpn:` + brand + `:` + MPN | Brand and MPN each non-empty, no leading or trailing ASCII whitespace, no ASCII uppercase letters. In each part, `%` is written `%25` and `:` is written `%3a`, so the key has exactly one `:` after `mpn:`. |
//! | `rec:` + 64 lowercase hex | Root ID of a product record. |
//!
//! "ASCII whitespace" is U+0009, U+000A, U+000C, U+000D, and U+0020, as in
//! the [WHATWG Infra standard](https://infra.spec.whatwg.org/#ascii-whitespace).

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::ids::{is_lower_hex, RecordId};

/// Why a string is not a canonical product key or a valid GTIN.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductKeyError {
    /// The key does not start with `gtin:`, `mpn:`, or `rec:`.
    #[error("product key must start with gtin:, mpn:, or rec:")]
    UnknownScheme,
    /// A GTIN is not 8, 12, 13, or 14 ASCII digits (or a key GTIN is not 14).
    #[error("GTIN must be 8, 12, 13, or 14 digits (14 in a product key)")]
    GtinFormat,
    /// The GTIN check digit is wrong.
    #[error("GTIN check digit is wrong")]
    GtinCheckDigit,
    /// An `mpn:` key is not in normalized form.
    #[error("mpn key must be mpn:<brand>:<mpn>, both non-empty, trimmed, and without ASCII uppercase")]
    MpnFormat,
    /// A `rec:` key does not hold 64 lowercase hex characters.
    #[error("rec key must be rec: followed by 64 lowercase hex characters")]
    RecFormat,
}

/// Which rule of ARCHITECTURE 4.8 produced a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyScheme {
    /// `gtin:` + 14 digits.
    Gtin,
    /// `mpn:` + brand + `:` + MPN.
    Mpn,
    /// `rec:` + root ID.
    Rec,
}

/// A canonical product key. Ordered and compared as a string.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProductKey(String);

const MAX_MPN_PART_CHARS: usize = 100;

fn is_ascii_ws(b: u8) -> bool {
    matches!(b, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

/// Trims ASCII whitespace from both ends (ARCHITECTURE 4.8).
pub fn trim_ascii_ws(s: &str) -> &str {
    let b = s.as_bytes();
    let mut start = 0;
    let mut end = b.len();
    while start < end && is_ascii_ws(b[start]) {
        start += 1;
    }
    while end > start && is_ascii_ws(b[end - 1]) {
        end -= 1;
    }
    &s[start..end]
}

/// Normalizes one part of an `mpn:` key: trim ASCII whitespace, lowercase
/// ASCII letters. Other characters are unchanged. The result is unescaped;
/// [`escape_mpn_part`] produces the form used inside a key.
pub fn normalize_mpn_part(s: &str) -> String {
    trim_ascii_ws(s).to_ascii_lowercase()
}

/// Escapes one normalized part for use inside an `mpn:` key: `%` becomes
/// `%25` and `:` becomes `%3a`. Without this, brand `a:b` with MPN `c` and
/// brand `a` with MPN `b:c` would share the key `mpn:a:b:c`.
pub fn escape_mpn_part(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '%' => out.push_str("%25"),
            ':' => out.push_str("%3a"),
            _ => out.push(c),
        }
    }
    out
}

/// Reverses [`escape_mpn_part`]. Returns `None` for any `%` not followed by
/// `25` or `3a` (lowercase), so each part has exactly one valid spelling.
pub fn unescape_mpn_part(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("%25") {
            out.push('%');
        } else if tail.starts_with("%3a") {
            out.push(':');
        } else {
            return None;
        }
        rest = &tail[3..];
    }
    out.push_str(rest);
    Some(out)
}

/// True if `p` is a valid escaped part of a canonical `mpn:` key.
fn escaped_mpn_part_ok(p: &str) -> bool {
    !p.contains(':') && unescape_mpn_part(p).is_some_and(|raw| mpn_part_ok(&raw))
}

fn mpn_part_ok(p: &str) -> bool {
    !p.is_empty()
        && p.chars().count() <= MAX_MPN_PART_CHARS
        && trim_ascii_ws(p).len() == p.len()
        && !p.bytes().any(|b| b.is_ascii_uppercase())
}

/// True if `digits` (ASCII digits only) has a valid GS1 check digit.
fn gtin_check_ok(digits: &[u8]) -> bool {
    let (body, check) = digits.split_at(digits.len() - 1);
    let mut sum = 0u32;
    for (i, &d) in body.iter().rev().enumerate() {
        let v = u32::from(d - b'0');
        sum += if i % 2 == 0 { 3 * v } else { v };
    }
    (10 - sum % 10) % 10 == u32::from(check[0] - b'0')
}

/// Validates a GTIN (8, 12, 13, or 14 ASCII digits with a valid check digit)
/// and returns it zero-padded to 14 digits.
pub fn normalize_gtin(gtin: &str) -> Result<String, ProductKeyError> {
    let b = gtin.as_bytes();
    if !matches!(b.len(), 8 | 12 | 13 | 14) || !b.iter().all(u8::is_ascii_digit) {
        return Err(ProductKeyError::GtinFormat);
    }
    if !gtin_check_ok(b) {
        return Err(ProductKeyError::GtinCheckDigit);
    }
    let mut s = "0".repeat(14 - b.len());
    s.push_str(gtin);
    Ok(s)
}

impl ProductKey {
    /// Parses and checks a key string. Only canonical keys are accepted.
    pub fn parse(s: &str) -> Result<ProductKey, ProductKeyError> {
        if let Some(rest) = s.strip_prefix("gtin:") {
            if rest.len() != 14 || !rest.bytes().all(|b| b.is_ascii_digit()) {
                return Err(ProductKeyError::GtinFormat);
            }
            if !gtin_check_ok(rest.as_bytes()) {
                return Err(ProductKeyError::GtinCheckDigit);
            }
        } else if let Some(rest) = s.strip_prefix("mpn:") {
            // Parts escape ':' (escape_mpn_part), so exactly one ':' separates them.
            let ok = match rest.split_once(':') {
                Some((b, m)) => escaped_mpn_part_ok(b) && escaped_mpn_part_ok(m),
                None => false,
            };
            if !ok {
                return Err(ProductKeyError::MpnFormat);
            }
        } else if let Some(rest) = s.strip_prefix("rec:") {
            if !is_lower_hex(rest, 64) {
                return Err(ProductKeyError::RecFormat);
            }
        } else {
            return Err(ProductKeyError::UnknownScheme);
        }
        Ok(ProductKey(s.to_owned()))
    }

    /// `gtin:` key from a GTIN of 8, 12, 13, or 14 digits.
    pub fn from_gtin(gtin: &str) -> Result<ProductKey, ProductKeyError> {
        Ok(ProductKey(format!("gtin:{}", normalize_gtin(gtin)?)))
    }

    /// `mpn:` key from a brand and MPN, normalized per ARCHITECTURE 4.8.
    pub fn from_brand_mpn(brand: &str, mpn: &str) -> Result<ProductKey, ProductKeyError> {
        let b = normalize_mpn_part(brand);
        let m = normalize_mpn_part(mpn);
        if !mpn_part_ok(&b) || !mpn_part_ok(&m) {
            return Err(ProductKeyError::MpnFormat);
        }
        Ok(ProductKey(format!("mpn:{}:{}", escape_mpn_part(&b), escape_mpn_part(&m))))
    }

    /// `rec:` key from a product chain's root ID.
    pub fn from_root(root: RecordId) -> ProductKey {
        ProductKey(format!("rec:{root}"))
    }

    /// Derives the key of a product (ARCHITECTURE 4.8, in priority order)
    /// from its `gtin`, `brand`, and `mpn` fields and its chain's root ID.
    pub fn derive(gtin: Option<&str>, brand: &str, mpn: Option<&str>, root: RecordId) -> Result<ProductKey, ProductKeyError> {
        if let Some(g) = gtin {
            return ProductKey::from_gtin(g);
        }
        if let Some(m) = mpn {
            return ProductKey::from_brand_mpn(brand, m);
        }
        Ok(ProductKey::from_root(root))
    }

    /// The key string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Which rule produced the key.
    pub fn scheme(&self) -> KeyScheme {
        if self.0.starts_with("gtin:") {
            KeyScheme::Gtin
        } else if self.0.starts_with("mpn:") {
            KeyScheme::Mpn
        } else {
            KeyScheme::Rec
        }
    }

    /// The 14-digit GTIN of a `gtin:` key.
    pub fn gtin(&self) -> Option<&str> {
        self.0.strip_prefix("gtin:")
    }

    /// The root ID of a `rec:` key.
    pub fn root(&self) -> Option<RecordId> {
        self.0.strip_prefix("rec:").and_then(|h| RecordId::from_hex(h).ok())
    }
}

impl fmt::Display for ProductKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for ProductKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ProductKey({})", self.0)
    }
}

impl FromStr for ProductKey {
    type Err = ProductKeyError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ProductKey::parse(s)
    }
}

impl AsRef<str> for ProductKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for ProductKey {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ProductKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        ProductKey::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gtin_rules() {
        // Known valid GTINs: EAN-13 4006381333931, UPC-A 036000291452, EAN-8 96385074.
        assert_eq!(normalize_gtin("4006381333931").unwrap(), "04006381333931");
        assert_eq!(normalize_gtin("036000291452").unwrap(), "00036000291452");
        assert_eq!(normalize_gtin("96385074").unwrap(), "00000096385074");
        assert_eq!(normalize_gtin("00012345678905").unwrap(), "00012345678905");
        assert_eq!(normalize_gtin("4006381333932"), Err(ProductKeyError::GtinCheckDigit));
        for bad in ["", "1234567", "123456789", "12345678901", "123456789012345", "40063813339a1", " 4006381333931", "４００６３８１３３３９３１"] {
            assert_eq!(normalize_gtin(bad), Err(ProductKeyError::GtinFormat), "{bad}");
        }
    }

    #[test]
    fn key_parse() {
        assert_eq!(ProductKey::parse("gtin:04006381333931").unwrap().scheme(), KeyScheme::Gtin);
        assert_eq!(ProductKey::parse("gtin:4006381333931"), Err(ProductKeyError::GtinFormat));
        assert_eq!(ProductKey::parse("gtin:04006381333932"), Err(ProductKeyError::GtinCheckDigit));
        assert_eq!(ProductKey::parse("mpn:acme:x-100").unwrap().scheme(), KeyScheme::Mpn);
        assert!(ProductKey::parse("mpn:sony:wh-1000xm5").is_ok());
        assert!(ProductKey::parse("mpn:müller:é1").is_ok());
        assert!(ProductKey::parse("mpn:a%3ab:c").is_ok()); // brand "a:b", mpn "c"
        assert!(ProductKey::parse("mpn:a:b%3ac").is_ok()); // brand "a", mpn "b:c"
        assert!(ProductKey::parse("mpn:100%25 cotton:x").is_ok());
        for bad in [
            "mpn:Acme:x", "mpn:acme:X", "mpn::x", "mpn:acme:", "mpn:acme", "mpn: acme:x", "mpn:acme:x ", "mpn:",
            "mpn:a:b:c", "mpn:a%3Ab:c", "mpn:a%:b", "mpn:a%2:b", "mpn:a%41:b", "mpn:%20a:b",
        ] {
            assert_eq!(ProductKey::parse(bad), Err(ProductKeyError::MpnFormat), "{bad}");
        }
        let root = RecordId([0xab; 32]);
        let k = ProductKey::parse(&format!("rec:{root}")).unwrap();
        assert_eq!(k.root(), Some(root));
        assert_eq!(ProductKey::parse(&format!("rec:{}", root.to_hex().to_uppercase())), Err(ProductKeyError::RecFormat));
        assert_eq!(ProductKey::parse("rec:ab"), Err(ProductKeyError::RecFormat));
        assert_eq!(ProductKey::parse("sku:1"), Err(ProductKeyError::UnknownScheme));
        assert_eq!(ProductKey::parse("GTIN:04006381333931"), Err(ProductKeyError::UnknownScheme));
    }

    #[test]
    fn derive_priority() {
        let root = RecordId([1; 32]);
        assert_eq!(ProductKey::derive(Some("4006381333931"), "Acme", Some("X1"), root).unwrap().as_str(), "gtin:04006381333931");
        assert_eq!(ProductKey::derive(None, "  ACME Corp\t", Some(" WH-1000XM5 "), root).unwrap().as_str(), "mpn:acme corp:wh-1000xm5");
        assert_eq!(ProductKey::derive(None, "Acme", None, root).unwrap(), ProductKey::from_root(root));
        // ASCII-only lowercasing: non-ASCII letters keep their case.
        assert_eq!(ProductKey::derive(None, "MÜLLER", Some("Ä1"), root).unwrap().as_str(), "mpn:mÜller:Ä1");
        assert_eq!(ProductKey::derive(None, "   ", Some("x"), root), Err(ProductKeyError::MpnFormat));
        // Every derived key parses.
        // Colons in brand or MPN cannot make two products share a key.
        let k1 = ProductKey::derive(None, "a:b", Some("c"), root).unwrap();
        let k2 = ProductKey::derive(None, "a", Some("b:c"), root).unwrap();
        assert_eq!(k1.as_str(), "mpn:a%3ab:c");
        assert_eq!(k2.as_str(), "mpn:a:b%3ac");
        assert_ne!(k1, k2);
        assert_eq!(ProductKey::derive(None, "100% Cotton", Some("X"), root).unwrap().as_str(), "mpn:100%25 cotton:x");
        for k in [
            k1,
            k2,
            ProductKey::derive(None, "a :b", Some("c"), root).unwrap(),
            ProductKey::derive(None, "MÜLLER", Some("Ä1"), root).unwrap(),
        ] {
            assert_eq!(ProductKey::parse(k.as_str()).unwrap(), k);
        }
    }

    #[test]
    fn serde_roundtrip() {
        let k = ProductKey::parse("mpn:acme:x").unwrap();
        let j = serde_json::to_string(&k).unwrap();
        assert_eq!(j, "\"mpn:acme:x\"");
        assert_eq!(serde_json::from_str::<ProductKey>(&j).unwrap(), k);
        assert!(serde_json::from_str::<ProductKey>("\"mpn:Acme:x\"").is_err());
    }
}
