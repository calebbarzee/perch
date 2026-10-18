//! Canonical JSON of view results (docs/views.md section 12.3): RFC 8785
//! key order and number form. Unlike `perch_core::to_canonical_json`, it
//! accepts the two-decimal numbers that views emit.

use serde::Serialize;
use serde_json::Value;

/// The RFC 8785 serialization of a view result.
///
/// Never panics: a value that does not serialize gives `null`, and a
/// non-finite number is written as `null`.
pub fn canonical_json<T: Serialize + ?Sized>(v: &T) -> String {
    let v = serde_json::to_value(v).unwrap_or(Value::Null);
    let mut out = String::new();
    write(&v, &mut out);
    out
}

fn write(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.push_str(&i.to_string());
            } else if let Some(u) = n.as_u64() {
                out.push_str(&u.to_string());
            } else {
                let f = n.as_f64().unwrap_or(f64::NAN);
                if !f.is_finite() {
                    out.push_str("null");
                    return;
                }
                // Rust's shortest round-trip form equals ECMAScript
                // Number::toString for |f| in [1e-6, 1e21), which covers
                // every view value (multiples of 0.01).
                if f == f.trunc() && f.abs() < 9.007_199_254_740_992e15 {
                    out.push_str(&(f as i64).to_string());
                } else {
                    out.push_str(&format!("{f}"));
                }
            }
        }
        Value::String(s) => perch_core::canonical::write_string(s, out),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(x, out);
            }
            out.push(']');
        }
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort_by(|a, b| perch_core::canonical::cmp_utf16(a, b));
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                perch_core::canonical::write_string(k, out);
                out.push(':');
                write(&m[k], out);
            }
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decimals_and_order() {
        let v = json!({ "b": 4.25, "a": [1, 0.07, -0.5], "\u{1F600}": 1, "\u{FF61}": 2 });
        // U+1F600 is a surrogate pair (D83D …) and sorts before U+FF61 in UTF-16.
        assert_eq!(canonical_json(&v), "{\"a\":[1,0.07,-0.5],\"b\":4.25,\"\u{1F600}\":1,\"\u{FF61}\":2}");
    }
}
