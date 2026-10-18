//! Canonical JSON encoding (ARCHITECTURE 4.2).
//!
//! The encoder follows [RFC 8785 (JCS)](https://www.rfc-editor.org/rfc/rfc8785)
//! restricted to integers:
//!
//! - no whitespace;
//! - object keys sorted by UTF-16 code units ([`cmp_utf16`]);
//! - integers only, within ±(2^53 − 1);
//! - strings escape only `"`, `\`, and U+0000–U+001F, using `\b \t \n \f \r`
//!   where defined and `\u00xx` (lowercase hex) otherwise.
//!
//! The encoder sorts keys itself, so the result does not depend on the key
//! order of the input map (for example when another crate enables the
//! `serde_json/preserve_order` feature).

use core::cmp::Ordering;
use core::fmt::Write as _;

use serde::Serialize;
use serde_json::Value;

use crate::limits::{MAX_DEPTH, MAX_SAFE_INTEGER};

/// Why a value has no canonical encoding.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum CanonicalError {
    /// A number is not an integer (it is a float in the `Value`).
    #[error("number {value} is not an integer")]
    NotInteger {
        /// The number as text.
        value: String,
    },
    /// An integer is outside ±(2^53 − 1).
    #[error("integer {value} is outside ±(2^53 - 1)")]
    OutOfRange {
        /// The number as text.
        value: String,
    },
    /// Nesting depth exceeds [`MAX_DEPTH`].
    #[error("nesting depth exceeds {max}")]
    TooDeep {
        /// Allowed depth.
        max: usize,
    },
}

/// Compares two strings by their UTF-16 code units, as RFC 8785 requires for
/// object keys. This differs from UTF-8 byte order only when one string has a
/// character at or above U+10000 where the other has one in U+E000–U+FFFF.
pub fn cmp_utf16(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        return a.as_bytes().cmp(b.as_bytes());
    }
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Encodes `v` canonically. Fails on floats, integers outside ±(2^53 − 1),
/// and nesting deeper than [`MAX_DEPTH`] (the outermost object or array is
/// depth 1).
pub fn to_canonical_json(v: &Value) -> Result<String, CanonicalError> {
    let mut out = String::new();
    write_canonical(v, &mut out)?;
    Ok(out)
}

/// Appends the canonical encoding of `v` to `out`. On error, `out` may hold
/// a partial encoding.
pub fn write_canonical(v: &Value, out: &mut String) -> Result<(), CanonicalError> {
    write_value(v, out, 0)
}

/// Canonical encoding of a JSON object given as a map.
pub fn map_to_canonical_json(m: &serde_json::Map<String, Value>) -> Result<String, CanonicalError> {
    let mut out = String::new();
    write_object(m, &mut out, 1)?;
    Ok(out)
}

fn write_value(v: &Value, out: &mut String, depth: usize) -> Result<(), CanonicalError> {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => write_number(n, out)?,
        Value::String(s) => write_string(s, out),
        Value::Array(a) => {
            if depth + 1 > MAX_DEPTH {
                return Err(CanonicalError::TooDeep { max: MAX_DEPTH });
            }
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(x, out, depth + 1)?;
            }
            out.push(']');
        }
        Value::Object(m) => write_object(m, out, depth + 1)?,
    }
    Ok(())
}

fn write_object(
    m: &serde_json::Map<String, Value>,
    out: &mut String,
    depth: usize,
) -> Result<(), CanonicalError> {
    if depth > MAX_DEPTH {
        return Err(CanonicalError::TooDeep { max: MAX_DEPTH });
    }
    let mut entries: Vec<(&String, &Value)> = m.iter().collect();
    entries.sort_by(|a, b| cmp_utf16(a.0, b.0));
    out.push('{');
    for (i, (k, x)) in entries.into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_string(k, out);
        out.push(':');
        write_value(x, out, depth)?;
    }
    out.push('}');
    Ok(())
}

fn write_number(n: &serde_json::Number, out: &mut String) -> Result<(), CanonicalError> {
    if let Some(u) = n.as_u64() {
        if u > MAX_SAFE_INTEGER {
            return Err(CanonicalError::OutOfRange { value: n.to_string() });
        }
        let _ = write!(out, "{u}");
        Ok(())
    } else if let Some(i) = n.as_i64() {
        if i.unsigned_abs() > MAX_SAFE_INTEGER {
            return Err(CanonicalError::OutOfRange { value: n.to_string() });
        }
        let _ = write!(out, "{i}");
        Ok(())
    } else {
        Err(CanonicalError::NotInteger { value: n.to_string() })
    }
}

/// Appends `s` as a canonical JSON string literal (with quotes).
pub fn write_string(s: &str, out: &mut String) {
    out.reserve(s.len() + 2);
    out.push('"');
    let bytes = s.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        let esc: Option<&str> = match b {
            b'"' => Some("\\\""),
            b'\\' => Some("\\\\"),
            0x08 => Some("\\b"),
            0x09 => Some("\\t"),
            0x0a => Some("\\n"),
            0x0c => Some("\\f"),
            0x0d => Some("\\r"),
            0x00..=0x1f => None,
            _ => continue,
        };
        out.push_str(&s[start..i]);
        match esc {
            Some(e) => out.push_str(e),
            None => {
                let _ = write!(out, "\\u{:04x}", b);
            }
        }
        start = i + 1;
    }
    out.push_str(&s[start..]);
    out.push('"');
}

/// Returns true if `text` is already in canonical form: it parses strictly
/// and re-encodes to the same bytes.
pub fn is_canonical(text: &str) -> bool {
    match crate::json::parse(text, MAX_DEPTH) {
        Ok(v) => to_canonical_json(&v).map(|c| c == text).unwrap_or(false),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn c(v: Value) -> String {
        to_canonical_json(&v).unwrap()
    }

    #[test]
    fn basic_encoding() {
        assert_eq!(c(json!({"b": 1, "a": [true, false, null], "c": "x"})), r#"{"a":[true,false,null],"b":1,"c":"x"}"#);
        assert_eq!(c(json!({})), "{}");
        assert_eq!(c(json!([])), "[]");
        assert_eq!(c(json!(-5)), "-5");
        assert_eq!(c(json!(9007199254740991u64)), "9007199254740991");
        assert_eq!(c(json!(-9007199254740991i64)), "-9007199254740991");
    }

    #[test]
    fn string_escapes() {
        assert_eq!(c(json!("a\"b\\c")), r#""a\"b\\c""#);
        assert_eq!(c(json!("\u{8}\t\n\u{c}\r")), r#""\b\t\n\f\r""#);
        assert_eq!(c(json!("\u{0}\u{1f}\u{1b}")), r#""\u0000\u001f\u001b""#);
        // Not escaped: '/', DEL, U+2028, non-ASCII.
        assert_eq!(c(json!("/\u{7f}\u{2028}é😀")), "\"/\u{7f}\u{2028}é😀\"");
    }

    #[test]
    fn utf16_key_order() {
        // U+E000 (UTF-8 EE 80 80) vs U+1F600 (UTF-8 F0 9F 98 80, UTF-16 D83D DE00).
        // UTF-8 order puts U+E000 first; UTF-16 order puts U+1F600 first.
        let v = json!({"\u{e000}": 1, "\u{1f600}": 2});
        assert_eq!(c(v), "{\"\u{1f600}\":2,\"\u{e000}\":1}");
        assert_eq!(cmp_utf16("\u{1f600}", "\u{e000}"), Ordering::Less);
        assert_eq!(cmp_utf16("a", "b"), Ordering::Less);
        assert_eq!(cmp_utf16("ab", "a"), Ordering::Greater);
        assert_eq!(cmp_utf16("body", "body_hash"), Ordering::Less);
    }

    #[test]
    fn rfc8785_sorting_example() {
        // Property names from RFC 8785 section 3.2.3.
        let text = r#"{"€":"Euro Sign","\r":"Carriage Return","דּ":"Hebrew Letter Dalet With Dagesh","1":"One","😀":"Emoji: Grinning Face","\u0080":"Control","ö":"Latin Small Letter O With Diaeresis"}"#;
        let v = crate::json::parse(text, 16).unwrap();
        let keys: Vec<String> = {
            let enc = c(v);
            let v2: serde_json::Map<String, Value> = serde_json::from_str(&enc).unwrap();
            let _ = v2;
            // Recover the key order from the encoding itself.
            let mut ks = Vec::new();
            let parsed = crate::json::parse(&enc, 16).unwrap();
            let mut entries: Vec<_> = parsed.as_object().unwrap().keys().cloned().collect();
            entries.sort_by(|a, b| enc.find(&format!("\"{a}\"")).cmp(&enc.find(&format!("\"{b}\""))));
            ks.extend(entries);
            ks
        };
        let expected = ["\r", "1", "\u{80}", "\u{f6}", "\u{20ac}", "\u{1f600}", "\u{fb33}"];
        assert_eq!(keys, expected);
    }

    #[test]
    fn rejects_floats_and_range() {
        assert!(matches!(to_canonical_json(&json!(1.5)), Err(CanonicalError::NotInteger { .. })));
        assert!(matches!(to_canonical_json(&json!(1.0)), Err(CanonicalError::NotInteger { .. })));
        assert!(matches!(to_canonical_json(&json!(9007199254740992u64)), Err(CanonicalError::OutOfRange { .. })));
        assert!(matches!(to_canonical_json(&json!(-9007199254740992i64)), Err(CanonicalError::OutOfRange { .. })));
        assert!(matches!(to_canonical_json(&json!(u64::MAX)), Err(CanonicalError::OutOfRange { .. })));
        assert!(matches!(to_canonical_json(&json!(i64::MIN)), Err(CanonicalError::OutOfRange { .. })));
    }

    #[test]
    fn depth_limit() {
        let mut v = json!(1);
        for _ in 0..16 {
            v = json!([v]);
        }
        assert!(to_canonical_json(&v).is_ok());
        v = json!({ "a": v });
        assert_eq!(to_canonical_json(&v), Err(CanonicalError::TooDeep { max: 16 }));
    }

    #[test]
    fn canonical_check() {
        assert!(is_canonical(r#"{"a":1,"b":[]}"#));
        assert!(!is_canonical(r#"{"b":[],"a":1}"#));
        assert!(!is_canonical(r#"{"a": 1}"#));
        let bs = char::from(0x5c);
        assert!(!is_canonical(&format!("\"{bs}u0041\"")));
        assert!(is_canonical(&format!("\"{bs}u001f\"")));
        assert!(!is_canonical(&format!("\"{bs}u001F\"")));
        assert!(!is_canonical(r#""\/""#));
    }
}
