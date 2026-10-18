//! Adversarial input vectors that do not depend on any Perch crate API.

use crate::fixture::{Log, Record, canonical, sha256_hex};
use serde_json::{Map, Value, json};

/// Ed25519 group order L, little-endian.
pub const L_LE: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
];

/// Encodings of small-order points (orders 1, 2, 4, 8), as used for weak keys.
pub const SMALL_ORDER_POINTS: [&str; 5] = [
    "0100000000000000000000000000000000000000000000000000000000000000", // identity, order 1
    "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f", // order 2
    "0000000000000000000000000000000000000000000000000000000000000000", // order 4
    "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05", // order 8
    "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a", // order 8
];

/// Add L to the S half of a signature. The result verifies under a
/// cofactorless equation that skips the `S < L` check, so strict verification
/// must reject it.
pub fn malleate_s(sig_hex: &str) -> String {
    let mut sig = hex::decode(sig_hex).unwrap();
    let mut carry = 0u16;
    for i in 0..32 {
        let v = sig[32 + i] as u16 + L_LE[i] as u16 + carry;
        sig[32 + i] = v as u8;
        carry = v >> 8;
    }
    assert_eq!(carry, 0);
    hex::encode(sig)
}

/// A record by the identity point as public key, with R = identity and S = 0.
/// The equation `[S]B = R + [k]A` holds for every message, so only a check that
/// rejects small-order keys stops it.
pub fn weak_key_record() -> String {
    let author = SMALL_ORDER_POINTS[0];
    let body = json!({ "name": "Weak", "role": "other" });
    let mut h = Map::new();
    h.insert("v".into(), json!(1));
    h.insert("author".into(), json!(author));
    h.insert("seq".into(), json!(1));
    h.insert("prev".into(), Value::Null);
    h.insert("ts".into(), json!(crate::fixture::T0));
    h.insert("kind".into(), json!("profile"));
    h.insert("body_hash".into(), json!(sha256_hex(canonical(&body).as_bytes())));
    let sig = format!("{}{}", SMALL_ORDER_POINTS[0], "00".repeat(32));
    let mut m = h;
    m.insert("sig".into(), json!(sig));
    m.insert("body".into(), body);
    canonical(&Value::Object(m))
}

/// Non-canonical but equivalent encodings of a valid record. Each must be
/// accepted and normalize to the same ID and canonical wire form.
pub fn equivalent_encodings(r: &Record) -> Vec<(&'static str, String)> {
    let wire = r.wire();
    let v = r.to_value();
    let mut out = vec![];

    // Pretty-printed with whitespace and newlines inside objects.
    out.push(("pretty", serde_json::to_string_pretty(&v).unwrap()));

    // Keys in reverse canonical order.
    out.push(("reversed keys", reversed_keys(&v)));

    // Spaces after separators.
    out.push(("spaces", wire.replace("\":", "\" : ").replace(",\"", " , \"")));

    // A letter escaped as A-style and `/` escaped as `\/` in a body string.
    let name = v["body"]["name"].as_str().unwrap_or("");
    if let Some(first) = name.chars().next().filter(|c| c.is_ascii_alphabetic()) {
        let esc = format!("\\u{:04x}{}", first as u32, &name[first.len_utf8()..]);
        out.push(("unicode escape", wire.replacen(&format!("\"{name}\""), &format!("\"{esc}\""), 1)));
        let esc_upper = format!("\\u{:04X}{}", first as u32, &name[first.len_utf8()..]);
        out.push(("unicode escape uppercase hex", wire.replacen(&format!("\"{name}\""), &format!("\"{esc_upper}\""), 1)));
    }
    out
}

fn reversed_keys(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort_by(|a, b| crate::fixture::utf16_cmp(b, a));
            let parts: Vec<String> = keys
                .iter()
                .map(|k| format!("{}:{}", canonical(&json!(k)), reversed_keys(&m[*k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(a) => format!("[{}]", a.iter().map(reversed_keys).collect::<Vec<_>>().join(",")),
        other => canonical(other),
    }
}

/// Encodings that must be rejected at the isolated level, with a label.
///
/// Each vector is built so that its only defect is the rule under test: the
/// signature and `body_hash` are valid for the value that a lax parser would
/// produce (last duplicate wins, `1.0` read as 1, unknown fields ignored, lone
/// surrogates replaced by U+FFFD, hex decoded case-insensitively). A lax
/// implementation therefore accepts these vectors, and a strict one rejects them.
pub fn invalid_encodings(log: &mut Log) -> Vec<(&'static str, String)> {
    use crate::fixture::{canonical_unchecked, header1, sign_raw_body, sign_record};
    let key = log.key.clone();
    let body = json!({ "name": "Valid", "role": "buyer", "about": "x", "n": 0 });
    let r = log.add("profile", body.clone());
    let wire = r.wire();
    let author = r.author().to_string();
    let mut out: Vec<(&'static str, String)> = vec![];

    // Duplicate key at the top level (same value, so the header is unchanged).
    out.push(("duplicate top-level key", wire.replacen("{", &format!("{{\"author\":\"{author}\","), 1)));

    // Duplicate body key; body_hash matches the last-wins reading.
    {
        let lax = json!({ "name": "Valid", "role": "buyer", "about": "y", "n": 0 });
        let rec = sign_record(&key, header1(&key, "profile"), Some(lax));
        let w = rec.wire().replacen("\"about\":\"y\"", "\"about\":\"x\",\"about\":\"y\"", 1);
        out.push(("duplicate body key", w));
    }

    // Number token forms that are not plain integers (signature is over `seq":N`).
    let seq = format!("\"seq\":{}", r.seq());
    out.push(("seq as N.0", wire.replacen(&seq, &format!("\"seq\":{}.0", r.seq()), 1)));
    out.push(("seq as Ne0", wire.replacen(&seq, &format!("\"seq\":{}e0", r.seq()), 1)));
    out.push(("body integer as -0", wire.replacen("\"n\":0", "\"n\":-0", 1)));

    // Lone surrogate; body_hash matches the U+FFFD-replacement reading.
    {
        let lax = json!({ "name": "Valid", "role": "buyer", "about": "x\u{fffd}", "n": 0 });
        let rec = sign_record(&key, header1(&key, "profile"), Some(lax));
        out.push(("lone surrogate", rec.wire().replacen("x\u{fffd}", "x\\ud800", 1)));
    }

    // Uppercase hex in author and sig (same bytes when decoded case-insensitively).
    out.push(("uppercase author hex", wire.replace(&author, &author.to_uppercase())));
    out.push(("uppercase sig hex", wire.replace(&r.sig, &r.sig.to_uppercase())));

    // `replaces: null` (same header when null is read as absent).
    out.push(("replaces null", r.wire_edited(|m| {
        m.insert("replaces".into(), Value::Null);
    })));

    // Unknown top-level field (ignored by a lax reader).
    out.push(("unknown top-level field", r.wire_edited(|m| {
        m.insert("extra".into(), json!(1));
    })));

    // Body that is not an object, with a matching body_hash.
    out.push(("body is array", sign_raw_body(&key, header1(&key, "profile"), "[1]").0));
    out.push(("body is string", sign_raw_body(&key, header1(&key, "profile"), "\"x\"").0));

    // Header without body_hash, signed as such, with a body attached.
    {
        let rec = sign_record(&key, header1(&key, "profile"), None);
        let mut v = rec.to_value();
        v["body"] = body.clone();
        out.push(("missing body_hash", canonical(&v)));
    }

    // Header field rules, each signed correctly.
    {
        let mut h = header1(&key, "profile");
        h.insert("v".into(), json!(2));
        out.push(("v = 2", sign_record(&key, h, Some(body.clone())).wire()));
    }
    {
        let mut h = header1(&key, "profile");
        h.insert("kind".into(), json!("Profile"));
        out.push(("kind uppercase", sign_record(&key, h, Some(body.clone())).wire()));
    }
    {
        let mut h = header1(&key, "profile");
        h.insert("kind".into(), json!("a".repeat(33)));
        out.push(("kind too long", sign_record(&key, h, Some(body.clone())).wire()));
    }
    {
        let mut h = header1(&key, "profile");
        h.insert("kind".into(), json!("1abc"));
        out.push(("kind starts with digit", sign_record(&key, h, Some(body.clone())).wire()));
    }
    {
        let mut h = header1(&key, "profile");
        h.insert("seq".into(), json!(0));
        out.push(("seq 0", sign_record(&key, h, Some(body.clone())).wire()));
    }
    {
        let mut h = header1(&key, "profile");
        h.insert("seq".into(), json!(2));
        out.push(("seq 2 with prev null", sign_record(&key, h, Some(body.clone())).wire()));
    }
    {
        let mut h = header1(&key, "profile");
        h.insert("prev".into(), json!(r.id));
        out.push(("seq 1 with prev set", sign_record(&key, h, Some(body.clone())).wire()));
    }
    {
        let mut h = header1(&key, "profile");
        h.insert("ts".into(), json!(-1));
        out.push(("negative ts", sign_record(&key, h, Some(body.clone())).wire()));
    }
    {
        let mut h = header1(&key, "profile");
        h.insert("replaces".into(), json!("abc"));
        out.push(("replaces not 64 hex", sign_record(&key, h, Some(body.clone())).wire()));
    }

    // Integer beyond 2^53 − 1 in the body, with a matching body_hash.
    {
        let big = json!({ "name": "Valid", "role": "buyer", "n": 9_007_199_254_740_992u64 });
        out.push(("integer 2^53", sign_raw_body(&key, header1(&key, "profile"), &canonical_unchecked(&big)).0));
    }

    // Not JSON / trailing garbage.
    out.push(("trailing garbage", format!("{wire}x")));
    out.push(("truncated", wire[..wire.len() - 1].to_string()));
    out.push(("top-level array", format!("[{wire}]")));
    out
}

/// A body nested to the given total record depth (record object = depth 1).
pub fn nested_body(record_depth: usize) -> Value {
    // record = 1, body = 2, so the body's inner value adds record_depth - 2 levels.
    let mut v = json!(0);
    for _ in 0..record_depth.saturating_sub(2) {
        v = json!([v]);
    }
    json!({ "name": "Deep", "role": "other", "x": v })
}

/// A body of roughly `bytes` canonical bytes made of many small keys.
pub fn wide_body(bytes: usize) -> Value {
    let mut m = Map::new();
    m.insert("name".into(), json!("Wide"));
    m.insert("role".into(), json!("other"));
    let mut i = 0usize;
    while canonical(&Value::Object(m.clone())).len() + 16 < bytes {
        m.insert(format!("k{i:06}"), json!(i as i64));
        i += 1;
    }
    Value::Object(m)
}

/// A body whose canonical form is exactly `bytes` long (if `bytes` ≥ 40).
pub fn body_of_len(bytes: usize) -> Value {
    let base = json!({ "name": "Big", "role": "other", "about": "" });
    let pad = bytes - canonical(&base).len();
    json!({ "name": "Big", "role": "other", "about": "a".repeat(pad) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{RECORD_DOMAIN, Log};
    use ed25519_dalek::{Signature, VerifyingKey};

    fn header_bytes(wire: &str) -> (VerifyingKey, Signature, Vec<u8>) {
        let mut v: Value = serde_json::from_str(wire).unwrap();
        let m = v.as_object_mut().unwrap();
        let sig = hex::decode(m.remove("sig").unwrap().as_str().unwrap()).unwrap();
        m.remove("body");
        let author: [u8; 32] = hex::decode(m["author"].as_str().unwrap()).unwrap().try_into().unwrap();
        let mut msg = RECORD_DOMAIN.to_vec();
        msg.extend_from_slice(canonical(&v).as_bytes());
        (
            VerifyingKey::from_bytes(&author).unwrap(),
            Signature::from_bytes(&sig.try_into().unwrap()),
            msg,
        )
    }

    #[test]
    fn weak_key_vector_passes_lax_and_fails_strict() {
        let (pk, sig, msg) = header_bytes(&weak_key_record());
        assert!(pk.is_weak());
        assert!(pk.verify_strict(&msg, &sig).is_err());
        use ed25519_dalek::Verifier;
        assert!(pk.verify(&msg, &sig).is_ok(), "vector must be dangerous: lax verification accepts it");
    }

    #[test]
    fn malleated_s_vector_is_rejected_by_dalek() {
        let mut l = Log::new("mall");
        let r = l.profile("M", "buyer");
        let w = r.wire().replace(&r.sig, &malleate_s(&r.sig));
        let (pk, sig, msg) = header_bytes(&w);
        assert!(pk.verify_strict(&msg, &sig).is_err());
    }

    #[test]
    fn size_helpers_are_exact() {
        assert_eq!(canonical(&body_of_len(65_536)).len(), 65_536);
        assert!(canonical(&wide_body(60_000)).len() <= 60_000);
    }
}

#[cfg(test)]
mod lax_tests {
    //! Each invalid vector must pass a deliberately lax verifier, so that the
    //! real test rejects it for the intended rule and not for a side effect.
    use super::*;
    use crate::fixture::{Log, RECORD_DOMAIN};
    use ed25519_dalek::{Signature, VerifyingKey};

    /// Lax verifier: serde_json parsing (last duplicate wins), integers read from
    /// floats, unknown fields dropped, null `replaces` dropped, hex decoded in any
    /// case, U+FFFD for lone surrogates, no field rules. Checks only signature and
    /// body_hash over the lax reading.
    fn lax_verify(line: &str) -> bool {
        let fixed = line.replace("\\ud800", "\u{fffd}");
        let Ok(mut v) = serde_json::from_str::<Value>(&fixed) else { return false };
        let Some(m) = v.as_object_mut() else { return false };
        let sig = m.remove("sig").and_then(|s| s.as_str().map(|s| s.to_lowercase()));
        let body = m.remove("body");
        m.remove("extra");
        if m.get("replaces") == Some(&Value::Null) {
            m.remove("replaces");
        }
        for f in ["seq", "v", "ts"] {
            if let Some(x) = m.get(f).and_then(|x| x.as_f64()) {
                m.insert(f.into(), json!(x as i64));
            }
        }
        if let Some(a) = m.get("author").and_then(|a| a.as_str()) {
            let lower = a.to_lowercase();
            m.insert("author".into(), json!(lower));
        }
        if let (Some(b), Some(h)) = (&body, m.get("body_hash")) {
            let raw = match b {
                Value::Object(_) | Value::Array(_) | Value::String(_) => {
                    crate::fixture::canonical_unchecked(&lax_ints(b.clone()))
                }
                _ => return false,
            };
            if sha256_hex(raw.as_bytes()) != h.as_str().unwrap_or("") {
                return false;
            }
        }
        let author: [u8; 32] = match hex::decode(m["author"].as_str().unwrap_or("")) {
            Ok(b) if b.len() == 32 => b.try_into().unwrap(),
            _ => return false,
        };
        let Ok(sigb) = hex::decode(sig.unwrap_or_default()) else { return false };
        let Ok(sigb) = <[u8; 64]>::try_from(sigb) else { return false };
        let mut msg = RECORD_DOMAIN.to_vec();
        msg.extend_from_slice(crate::fixture::canonical_unchecked(&v).as_bytes());
        VerifyingKey::from_bytes(&author)
            .map(|k| k.verify_strict(&msg, &Signature::from_bytes(&sigb)).is_ok())
            .unwrap_or(false)
    }

    fn lax_ints(v: Value) -> Value {
        match v {
            Value::Number(n) if n.is_f64() => json!(n.as_f64().unwrap() as i64),
            Value::Array(a) => Value::Array(a.into_iter().map(lax_ints).collect()),
            Value::Object(m) => Value::Object(m.into_iter().map(|(k, v)| (k, lax_ints(v))).collect()),
            o => o,
        }
    }

    #[test]
    fn invalid_vectors_isolate_one_defect() {
        let mut l = Log::new("lax");
        // Vectors that are not JSON at all are excluded.
        let structural = ["trailing garbage", "truncated", "top-level array"];
        for (label, w) in invalid_encodings(&mut l) {
            if structural.contains(&label) {
                continue;
            }
            assert!(lax_verify(&w), "vector '{label}' should pass a lax verifier: {w}");
        }
    }
}
