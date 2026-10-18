//! Strict JSON parser (ARCHITECTURE 4.2, "Input parsing rules").
//!
//! [`parse`] accepts [RFC 8259](https://www.rfc-editor.org/rfc/rfc8259) JSON
//! and additionally rejects:
//!
//! - duplicate object keys at any depth;
//! - number tokens with a fraction or exponent (`1.0`, `1e0`), `-0`, and
//!   integers outside ±(2^53 − 1);
//! - `\u` escapes that form a lone surrogate;
//! - nesting deeper than `max_depth` (the outermost object or array is depth 1;
//!   each nested object or array adds 1);
//! - a byte order mark or any data after the value.
//!
//! The result is a [`serde_json::Value`]. Its object key order is irrelevant:
//! [`crate::canonical`] sorts keys itself.

use serde::Serialize;
use serde_json::{Map, Number, Value};

use crate::limits::MAX_SAFE_INTEGER;

/// Why strict parsing failed. `offset` is a byte offset into the input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum JsonError {
    /// Input is larger than the caller's limit; checked before parsing.
    #[error("input is {actual} bytes; limit is {limit}")]
    TooLarge {
        /// Input size in bytes.
        actual: usize,
        /// Limit in bytes.
        limit: usize,
    },
    /// Malformed JSON.
    #[error("invalid JSON at byte {offset}: {message}")]
    Syntax {
        /// Byte offset of the problem.
        offset: usize,
        /// What was expected.
        message: &'static str,
    },
    /// The same key appears twice in one object.
    #[error("duplicate key {key:?} at byte {offset}")]
    DuplicateKey {
        /// The repeated key.
        key: String,
        /// Byte offset of the second occurrence.
        offset: usize,
    },
    /// Objects and arrays are nested deeper than allowed.
    #[error("nesting depth exceeds {max} at byte {offset}")]
    TooDeep {
        /// Allowed depth.
        max: usize,
        /// Byte offset of the opening bracket that exceeded it.
        offset: usize,
    },
    /// A number token has a fraction or exponent.
    #[error("number at byte {offset} is not an integer token")]
    NotInteger {
        /// Byte offset of the number.
        offset: usize,
    },
    /// The number token `-0`.
    #[error("number at byte {offset} is -0")]
    NegativeZero {
        /// Byte offset of the number.
        offset: usize,
    },
    /// An integer outside ±(2^53 − 1).
    #[error("integer at byte {offset} is outside ±(2^53 - 1)")]
    OutOfRange {
        /// Byte offset of the number.
        offset: usize,
    },
    /// A `\u` escape forms an unpaired UTF-16 surrogate.
    #[error("lone surrogate escape at byte {offset}")]
    LoneSurrogate {
        /// Byte offset of the escape.
        offset: usize,
    },
}

/// Parses `input` strictly. See the module documentation for the rules.
pub fn parse(input: &str, max_depth: usize) -> Result<Value, JsonError> {
    let mut p = Parser { s: input.as_bytes(), i: 0, max_depth };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.syntax("unexpected data after the JSON value"));
    }
    Ok(v)
}

/// Like [`parse`], but first rejects input longer than `max_bytes`.
pub fn parse_limited(input: &str, max_bytes: usize, max_depth: usize) -> Result<Value, JsonError> {
    if input.len() > max_bytes {
        return Err(JsonError::TooLarge { actual: input.len(), limit: max_bytes });
    }
    parse(input, max_depth)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    max_depth: usize,
}

impl Parser<'_> {
    fn syntax(&self, message: &'static str) -> JsonError {
        JsonError::Syntax { offset: self.i, message }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.i += 1;
        }
    }

    fn expect_lit(&mut self, lit: &'static [u8], v: Value) -> Result<Value, JsonError> {
        if self.s[self.i..].starts_with(lit) {
            self.i += lit.len();
            Ok(v)
        } else {
            Err(self.syntax("invalid literal"))
        }
    }

    /// `depth` is the number of enclosing objects and arrays.
    fn value(&mut self, depth: usize) -> Result<Value, JsonError> {
        match self.peek() {
            None => Err(self.syntax("unexpected end of input")),
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => self.array(depth + 1),
            Some(b'"') => Ok(Value::String(self.string()?)),
            Some(b't') => self.expect_lit(b"true", Value::Bool(true)),
            Some(b'f') => self.expect_lit(b"false", Value::Bool(false)),
            Some(b'n') => self.expect_lit(b"null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.syntax("expected a JSON value")),
        }
    }

    fn check_depth(&self, depth: usize) -> Result<(), JsonError> {
        if depth > self.max_depth {
            Err(JsonError::TooDeep { max: self.max_depth, offset: self.i })
        } else {
            Ok(())
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, JsonError> {
        self.check_depth(depth)?;
        self.i += 1; // '{'
        let mut map = Map::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Value::Object(map));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.syntax("expected an object key string"));
            }
            let key_offset = self.i;
            let key = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err(self.syntax("expected ':' after object key"));
            }
            self.i += 1;
            self.ws();
            let v = self.value(depth)?;
            if map.contains_key(&key) {
                return Err(JsonError::DuplicateKey { key, offset: key_offset });
            }
            map.insert(key, v);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Value::Object(map));
                }
                _ => return Err(self.syntax("expected ',' or '}' in object")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, JsonError> {
        self.check_depth(depth)?;
        self.i += 1; // '['
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.ws();
            items.push(self.value(depth)?);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.syntax("expected ',' or ']' in array")),
            }
        }
    }

    fn number(&mut self) -> Result<Value, JsonError> {
        let start = self.i;
        let neg = self.peek() == Some(b'-');
        if neg {
            self.i += 1;
        }
        let int_start = self.i;
        match self.peek() {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                while let Some(b'0'..=b'9') = self.peek() {
                    self.i += 1;
                }
            }
            _ => return Err(self.syntax("expected a digit")),
        }
        let int_end = self.i;
        let mut not_integer = false;
        if self.peek() == Some(b'.') {
            self.i += 1;
            let d = self.i;
            while let Some(b'0'..=b'9') = self.peek() {
                self.i += 1;
            }
            if self.i == d {
                return Err(self.syntax("expected a digit after '.'"));
            }
            not_integer = true;
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.i += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.i += 1;
            }
            let d = self.i;
            while let Some(b'0'..=b'9') = self.peek() {
                self.i += 1;
            }
            if self.i == d {
                return Err(self.syntax("expected a digit in exponent"));
            }
            not_integer = true;
        }
        if not_integer {
            return Err(JsonError::NotInteger { offset: start });
        }
        let digits = &self.s[int_start..int_end];
        if neg && digits == b"0" {
            return Err(JsonError::NegativeZero { offset: start });
        }
        // 2^53 - 1 has 16 digits.
        if digits.len() > 16 {
            return Err(JsonError::OutOfRange { offset: start });
        }
        let mut n: u64 = 0;
        for &d in digits {
            n = n * 10 + u64::from(d - b'0');
        }
        if n > MAX_SAFE_INTEGER {
            return Err(JsonError::OutOfRange { offset: start });
        }
        let num = if neg { Number::from(-(n as i64)) } else { Number::from(n) };
        Ok(Value::Number(num))
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        if self.i + 4 > self.s.len() {
            return Err(self.syntax("expected 4 hex digits after \\u"));
        }
        let mut v = 0u32;
        for k in 0..4 {
            let c = self.s[self.i + k];
            let d = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => return Err(self.syntax("expected 4 hex digits after \\u")),
            };
            v = (v << 4) | u32::from(d);
        }
        self.i += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.i += 1; // opening quote
        let mut out = String::new();
        loop {
            // Copy a run of plain bytes. The input is valid UTF-8 and the run
            // stops only at ASCII bytes, so each run is valid UTF-8.
            let run_start = self.i;
            while let Some(c) = self.peek() {
                if c == b'"' || c == b'\\' || c < 0x20 {
                    break;
                }
                self.i += 1;
            }
            if self.i > run_start {
                // SAFETY-free: from_utf8 cannot fail here, but stay checked.
                match core::str::from_utf8(&self.s[run_start..self.i]) {
                    Ok(t) => out.push_str(t),
                    Err(_) => return Err(self.syntax("invalid UTF-8")),
                }
            }
            match self.peek() {
                None => return Err(self.syntax("unterminated string")),
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    let esc_start = self.i;
                    self.i += 1;
                    let c = self.peek().ok_or_else(|| self.syntax("unterminated escape"))?;
                    self.i += 1;
                    match c {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{08}'),
                        b'f' => out.push('\u{0c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let cp = if (0xD800..0xDC00).contains(&hi) {
                                if self.s[self.i..].starts_with(b"\\u") {
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    if !(0xDC00..0xE000).contains(&lo) {
                                        return Err(JsonError::LoneSurrogate { offset: esc_start });
                                    }
                                    0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                                } else {
                                    return Err(JsonError::LoneSurrogate { offset: esc_start });
                                }
                            } else if (0xDC00..0xE000).contains(&hi) {
                                return Err(JsonError::LoneSurrogate { offset: esc_start });
                            } else {
                                hi
                            };
                            match char::from_u32(cp) {
                                Some(ch) => out.push(ch),
                                None => return Err(JsonError::LoneSurrogate { offset: esc_start }),
                            }
                        }
                        _ => {
                            self.i = esc_start;
                            return Err(self.syntax("invalid escape"));
                        }
                    }
                }
                Some(_) => return Err(self.syntax("control character in string")),
            }
        }
    }
}

/// Depth of a value: scalars are 0, an object or array is 1 plus the
/// maximum depth of its members. Stops counting once `limit` is exceeded,
/// and returns `limit + 1` in that case.
pub fn depth(v: &Value, limit: usize) -> usize {
    fn go(v: &Value, here: usize, limit: usize) -> usize {
        if here > limit && (v.is_object() || v.is_array()) {
            return here;
        }
        match v {
            Value::Object(m) => {
                let mut best = here;
                for x in m.values() {
                    best = best.max(go(x, here + 1, limit));
                    if best > limit {
                        break;
                    }
                }
                best
            }
            Value::Array(a) => {
                let mut best = here;
                for x in a {
                    best = best.max(go(x, here + 1, limit));
                    if best > limit {
                        break;
                    }
                }
                best
            }
            _ => here - 1,
        }
    }
    go(v, 1, limit).min(limit + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn p(s: &str) -> Result<Value, JsonError> {
        parse(s, 16)
    }

    #[test]
    fn accepts_plain_json() {
        assert_eq!(p(r#" {"a": [1, -2, true, false, null, "x"]} "#).unwrap(), json!({"a":[1,-2,true,false,null,"x"]}));
        assert_eq!(p("0").unwrap(), json!(0));
        assert_eq!(p("9007199254740991").unwrap(), json!(9007199254740991u64));
        assert_eq!(p("-9007199254740991").unwrap(), json!(-9007199254740991i64));
        assert_eq!(p(r#""\u00e9\ud83d\ude00\/""#).unwrap(), json!("é😀/"));
    }

    #[test]
    fn rejects_duplicates_at_any_depth() {
        assert!(matches!(p(r#"{"a":1,"a":2}"#), Err(JsonError::DuplicateKey { .. })));
        assert!(matches!(p(r#"{"x":{"a":1,"a":1}}"#), Err(JsonError::DuplicateKey { .. })));
        assert!(matches!(p(r#"[{"b":[{"k":1,"k":2}]}]"#), Err(JsonError::DuplicateKey { .. })));
        // Escaped and unescaped spelling of the same key are duplicates.
        assert!(matches!(p(r#"{"a":1,"\u0061":2}"#), Err(JsonError::DuplicateKey { .. })));
    }

    #[test]
    fn rejects_non_integer_tokens() {
        for s in ["1.0", "1e0", "1E5", "0.5", "-1.5e3", "1e-1"] {
            assert!(matches!(p(s), Err(JsonError::NotInteger { .. })), "{s}");
        }
        assert!(matches!(p("-0"), Err(JsonError::NegativeZero { .. })));
        assert!(matches!(p("9007199254740992"), Err(JsonError::OutOfRange { .. })));
        assert!(matches!(p("-9007199254740992"), Err(JsonError::OutOfRange { .. })));
        assert!(matches!(p("123456789012345678901"), Err(JsonError::OutOfRange { .. })));
        for s in ["01", "-", "+1", "1.", ".5", "1e", "0x10", "NaN", "Infinity"] {
            assert!(p(s).is_err(), "{s}");
        }
    }

    #[test]
    fn rejects_lone_surrogates() {
        for s in [r#""\ud800""#, r#""\udc00""#, r#""\ud800x""#, r#""\ud800\u0041""#, r#""\ud800\ud800""#] {
            assert!(matches!(p(s), Err(JsonError::LoneSurrogate { .. })), "{s}");
        }
    }

    #[test]
    fn depth_limit() {
        let ok = "[".repeat(16) + &"]".repeat(16);
        assert!(p(&ok).is_ok());
        let bad = "[".repeat(17) + &"]".repeat(17);
        assert!(matches!(p(&bad), Err(JsonError::TooDeep { max: 16, .. })));
        let obj_ok = r#"{"a":"#.repeat(15) + "{}" + &"}".repeat(15);
        assert!(p(&obj_ok).is_ok());
        let obj_bad = r#"{"a":"#.repeat(16) + "{}" + &"}".repeat(16);
        assert!(matches!(p(&obj_bad), Err(JsonError::TooDeep { .. })));
        assert_eq!(depth(&p(&ok).unwrap(), 16), 16);
        assert_eq!(depth(&p(&obj_ok).unwrap(), 16), 16);
        assert_eq!(depth(&json!(1), 16), 0);
        assert_eq!(depth(&json!({"a":[{}]}), 16), 3);
        // A very deep value is cut off at limit + 1.
        assert_eq!(depth(&p(&ok).unwrap(), 3), 4);
        // A scalar inside the deepest allowed container does not add depth.
        assert_eq!(depth(&json!([[1]]), 2), 2);
        assert_eq!(depth(&json!([[[1]]]), 2), 3);
        assert_eq!(depth(&json!({"a": {"b": "x"}}), 2), 2);
    }

    #[test]
    fn rejects_syntax_errors() {
        for s in [
            "", " ", "{", "}", "[1,]", "{\"a\":1,}", "{\"a\"}", "{a:1}", "'a'", "\"a", "\"\\x\"", "\"\\u12\"",
            "\"a\u{1}\"", "tru", "nul", "1 2", "{} {}", "\u{feff}{}", "[1]x",
        ] {
            assert!(p(s).is_err(), "{s:?}");
        }
    }

    #[test]
    fn size_limit_checked_first() {
        assert!(matches!(parse_limited("[1,2]", 4, 16), Err(JsonError::TooLarge { actual: 5, limit: 4 })));
        assert!(parse_limited("[1,2]", 5, 16).is_ok());
    }
}
