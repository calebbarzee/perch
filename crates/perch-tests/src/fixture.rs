//! Independent record builder and signer (contract sections 4.1 to 4.3, 4.7).

use ed25519_dalek::{Signer, SigningKey};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

/// Signature domain for record headers (contract 4.3).
pub const RECORD_DOMAIN: &[u8] = b"perch/record/v1\n";
/// Signature domain for detached receipts (contract 4.3).
pub const RECEIPT_DOMAIN: &[u8] = b"perch/receipt/v1\n";
/// Largest integer allowed in canonical JSON (contract 4.2).
pub const MAX_SAFE: i64 = (1 << 53) - 1;
/// Base timestamp used by fixtures: 2025-10-09T08:53:20Z.
pub const T0: i64 = 1_760_000_000_000;

// ---------------------------------------------------------------------------
// Canonical JSON (RFC 8785 restricted to integers)
// ---------------------------------------------------------------------------

/// Canonical JSON of a value. Panics on fractional numbers or integers outside
/// ±(2^53 − 1), because fixtures never produce them on purpose.
pub fn canonical(v: &Value) -> String {
    let mut out = String::new();
    write_canonical(v, &mut out, true);
    out
}

/// Canonical JSON that allows any integer serde_json can hold. Used only to
/// build vectors whose single defect is an out-of-range integer.
pub fn canonical_unchecked(v: &Value) -> String {
    let mut out = String::new();
    write_canonical(v, &mut out, false);
    out
}

fn write_canonical(v: &Value, out: &mut String, checked: bool) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if !checked {
                assert!(n.is_i64() || n.is_u64(), "integers only");
                out.push_str(&n.to_string());
                return;
            }
            let i = n
                .as_i64()
                .unwrap_or_else(|| panic!("fixture produced non-integer number {n}"));
            assert!((-MAX_SAFE..=MAX_SAFE).contains(&i), "integer out of range: {i}");
            out.push_str(&i.to_string());
        }
        Value::String(s) => write_string(s, out),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(x, out, checked);
            }
            out.push(']');
        }
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort_by(|a, b| utf16_cmp(a, b));
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(k, out);
                out.push(':');
                write_canonical(&m[*k], out, checked);
            }
            out.push('}');
        }
    }
}

/// Compare two strings by UTF-16 code units (RFC 8785 section 3.2.3).
pub fn utf16_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{0c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// A test key derived from a one-byte seed label, so tests are deterministic.
#[derive(Clone)]
pub struct Key {
    pub signing: SigningKey,
}

impl Key {
    pub fn from_label(label: &str) -> Key {
        let seed: [u8; 32] = Sha256::digest(format!("perch-tests/key/{label}").as_bytes()).into();
        Key { signing: SigningKey::from_bytes(&seed) }
    }

    pub fn author(&self) -> String {
        hex::encode(self.signing.verifying_key().to_bytes())
    }

    pub fn secret_hex(&self) -> String {
        hex::encode(self.signing.to_bytes())
    }

    pub fn sign_hex(&self, domain: &[u8], msg: &[u8]) -> String {
        let mut m = Vec::with_capacity(domain.len() + msg.len());
        m.extend_from_slice(domain);
        m.extend_from_slice(msg);
        hex::encode(self.signing.sign(&m).to_bytes())
    }
}

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// A signed record as built by the fixture.
#[derive(Clone, Debug)]
pub struct Record {
    pub header: Map<String, Value>,
    pub sig: String,
    pub body: Option<Value>,
    pub id: String,
}

impl Record {
    pub fn author(&self) -> &str {
        self.header["author"].as_str().unwrap()
    }
    pub fn seq(&self) -> i64 {
        self.header["seq"].as_i64().unwrap()
    }
    pub fn kind(&self) -> &str {
        self.header["kind"].as_str().unwrap()
    }

    /// Full record as a JSON object (header + sig + body).
    pub fn to_value(&self) -> Value {
        let mut m = self.header.clone();
        m.insert("sig".into(), Value::String(self.sig.clone()));
        if let Some(b) = &self.body {
            m.insert("body".into(), b.clone());
        }
        Value::Object(m)
    }

    /// Canonical wire form (contract 4.2): one JSONL line.
    pub fn wire(&self) -> String {
        canonical(&self.to_value())
    }

    /// Wire form after an arbitrary edit, without re-signing.
    pub fn wire_edited(&self, edit: impl FnOnce(&mut Map<String, Value>)) -> String {
        let mut v = self.to_value();
        edit(v.as_object_mut().unwrap());
        canonical(&v)
    }

    /// Same record with the body removed (contract 5.3).
    pub fn without_body(&self) -> Record {
        Record { body: None, ..self.clone() }
    }
}

/// Options for one record.
#[derive(Default, Clone)]
pub struct Opts {
    pub replaces: Option<String>,
    pub ts: Option<i64>,
    /// Force a seq (for forks); `prev` is then taken from `prev_override` or left as tracked.
    pub seq: Option<i64>,
    pub prev: Option<Option<String>>,
}

/// Sign an arbitrary header and body exactly as the contract defines.
pub fn sign_record(key: &Key, mut header: Map<String, Value>, body: Option<Value>) -> Record {
    if let Some(b) = &body {
        header.insert("body_hash".into(), Value::String(sha256_hex(canonical(b).as_bytes())));
    }
    let hc = canonical(&Value::Object(header.clone()));
    let id = sha256_hex(hc.as_bytes());
    let sig = key.sign_hex(RECORD_DOMAIN, hc.as_bytes());
    Record { header, sig, body, id }
}

/// Sign a header whose `body_hash` is computed over a raw body string, and
/// return the wire line `{header..., "sig", "body": <raw>}` with keys in
/// canonical order. Used for vectors whose body is not valid canonical JSON.
pub fn sign_raw_body(key: &Key, mut header: Map<String, Value>, body_raw: &str) -> (String, String) {
    header.insert("body_hash".into(), Value::String(sha256_hex(body_raw.as_bytes())));
    let hc = canonical(&Value::Object(header.clone()));
    let id = sha256_hex(hc.as_bytes());
    let sig = key.sign_hex(RECORD_DOMAIN, hc.as_bytes());
    // "body" sorts before every header key except "author"; build by hand.
    let mut m = header;
    m.insert("sig".into(), Value::String(sig));
    let without_body = canonical(&Value::Object(m));
    // Insert `"body":<raw>` after the "author" member, which sorts first.
    let author_end = without_body.find(",\"body_hash\"").unwrap();
    let line = format!("{},\"body\":{}{}", &without_body[..author_end], body_raw, &without_body[author_end..]);
    (line, id)
}

/// A standard header map for `key` at seq 1.
pub fn header1(key: &Key, kind: &str) -> Map<String, Value> {
    let mut h = Map::new();
    h.insert("v".into(), json!(1));
    h.insert("author".into(), json!(key.author()));
    h.insert("seq".into(), json!(1));
    h.insert("prev".into(), Value::Null);
    h.insert("ts".into(), json!(T0));
    h.insert("kind".into(), json!(kind));
    h
}

/// An author's log under construction.
#[derive(Clone)]
pub struct Log {
    pub key: Key,
    pub records: Vec<Record>,
    pub ts: i64,
}

impl Log {
    pub fn new(label: &str) -> Log {
        Log { key: Key::from_label(label), records: Vec::new(), ts: T0 }
    }

    pub fn author(&self) -> String {
        self.key.author()
    }

    pub fn head(&self) -> Option<&Record> {
        self.records.last()
    }

    /// Append a record with default options.
    pub fn add(&mut self, kind: &str, body: Value) -> Record {
        self.add_with(kind, body, Opts::default())
    }

    /// Append a record. With `opts.seq` set, the record is built at that seq
    /// but not appended (used to build forks).
    pub fn add_with(&mut self, kind: &str, body: Value, opts: Opts) -> Record {
        let forced = opts.seq.is_some();
        let seq = opts.seq.unwrap_or(self.records.len() as i64 + 1);
        let prev = match opts.prev {
            Some(p) => p,
            None => {
                if seq == 1 {
                    None
                } else {
                    Some(self.records[(seq - 2) as usize].id.clone())
                }
            }
        };
        let ts = opts.ts.unwrap_or_else(|| {
            self.ts += 1000;
            self.ts
        });
        let mut h = Map::new();
        h.insert("v".into(), json!(1));
        h.insert("author".into(), json!(self.author()));
        h.insert("seq".into(), json!(seq));
        h.insert("prev".into(), prev.map(Value::String).unwrap_or(Value::Null));
        h.insert("ts".into(), json!(ts));
        h.insert("kind".into(), json!(kind));
        if let Some(r) = opts.replaces {
            h.insert("replaces".into(), json!(r));
        }
        let rec = sign_record(&self.key, h, Some(body));
        if !forced {
            self.records.push(rec.clone());
        }
        rec
    }

    /// A different record at an existing seq (fork evidence type 1).
    pub fn fork_at(&self, seq: i64, kind: &str, body: Value) -> Record {
        let mut clone = self.clone();
        clone.add_with(kind, body, Opts { seq: Some(seq), ts: Some(T0 + 999 + seq), ..Default::default() })
    }

    pub fn jsonl(&self) -> String {
        self.records.iter().map(|r| r.wire() + "\n").collect()
    }

    // ---- kind helpers -----------------------------------------------------

    pub fn profile(&mut self, name: &str, role: &str) -> Record {
        self.add("profile", json!({ "name": name, "role": role }))
    }

    pub fn product_gtin(&mut self, name: &str, brand: &str, gtin: &str) -> Record {
        self.add("product", json!({ "name": name, "brand": brand, "gtin": gtin }))
    }

    pub fn listing(&mut self, product_key: &str, url: &str, amount: i64) -> Record {
        self.add(
            "listing",
            json!({
                "product": product_key,
                "url": url,
                "price": { "amount": amount, "currency": "EUR" },
                "availability": "in_stock",
                "condition": "new"
            }),
        )
    }

    pub fn review(&mut self, product_key: &str, rating: i64, receipt: Option<Value>) -> Record {
        let mut b = json!({ "product": product_key, "rating": rating, "text": "Review text." });
        if let Some(r) = receipt {
            b["receipt"] = r;
        }
        self.add("review", b)
    }

    pub fn flag(&mut self, target: &str, reason: &str) -> Record {
        self.add("flag", json!({ "target": target, "reason": reason }))
    }

    pub fn retract(&mut self, target: &str) -> Record {
        self.add("retract", json!({ "target": target }))
    }

    pub fn attest_identity(&mut self, subject: &str, person: &str, expires: i64) -> Record {
        self.add(
            "attest.identity",
            json!({ "subject": subject, "person": person_hex(person), "level": "person", "expires": expires }),
        )
    }

    pub fn attest_domain(&mut self, subject: &str, domain: &str, expires: i64) -> Record {
        self.add("attest.domain", json!({ "subject": subject, "domain": domain, "expires": expires }))
    }

    pub fn attest_revoke(&mut self, subject: &str, valid_through_seq: i64) -> Record {
        self.add("attest.revoke", json!({ "subject": subject, "valid_through_seq": valid_through_seq }))
    }

    /// Issue a detached receipt (contract 4.7) signed by this log's key.
    pub fn receipt(&self, listing_root: &str, buyer: &str, purchased_at: i64, nonce_label: &str) -> Value {
        receipt(&self.key, listing_root, buyer, purchased_at, nonce_label)
    }
}

/// A 64-hex pseudonym derived from a label.
pub fn person_hex(label: &str) -> String {
    sha256_hex(format!("perch-tests/person/{label}").as_bytes())
}

/// Build a detached receipt signed by `seller`.
pub fn receipt(seller: &Key, listing_root: &str, buyer: &str, purchased_at: i64, nonce_label: &str) -> Value {
    let nonce = sha256_hex(format!("perch-tests/nonce/{nonce_label}").as_bytes());
    let payload = json!({ "listing": listing_root, "buyer": buyer, "purchased_at": purchased_at, "nonce": nonce });
    let sig = seller.sign_hex(RECEIPT_DOMAIN, canonical(&payload).as_bytes());
    let mut r = payload;
    r["sig"] = json!(sig);
    r
}

/// GTIN-13 product key (contract 4.8): zero-padded to 14 digits.
pub fn gtin_key(gtin: &str) -> String {
    format!("gtin:{gtin:0>14}")
}

/// A default policy (contract 7.1) trusting the given identity and domain attestors.
pub fn policy(now: i64, identity_attestors: &[&str], domain_attestors: &[&str]) -> Value {
    let mut ta: Vec<Value> = identity_attestors
        .iter()
        .map(|a| json!({ "author": a, "kinds": ["attest.identity", "attest.revoke"], "not_after_seq": null }))
        .collect();
    ta.extend(
        domain_attestors
            .iter()
            .map(|a| json!({ "author": a, "kinds": ["attest.domain", "attest.revoke"], "not_after_seq": null })),
    );
    json!({
        "now": now,
        "trusted_attestors": ta,
        "flag_threshold": 3,
        "flag_thresholds": { "review": 5, "product": 5 },
        "flags_require_verified": true,
        "seller_hidden_listing_limit": 3,
        "seller_min_distinct_flaggers": 9,
        "verified_purchase_requires_verified_buyer": true,
        "price_requires_domain_verified": true,
        "future_ts_tolerance_ms": 600000
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_sorts_by_utf16_and_escapes() {
        // U+FF61 (BMP, UTF-16 0xFF61) sorts after U+1F600 (UTF-16 0xD83D...) in UTF-16,
        // but before it in UTF-8 bytes (EF BD A1 < F0 9F 98 80).
        let v = json!({ "\u{ff61}": 1, "\u{1f600}": 2, "a": "x\"\\\n\u{1}" });
        assert_eq!(canonical(&v), "{\"a\":\"x\\\"\\\\\\n\\u0001\",\"\u{1f600}\":2,\"\u{ff61}\":1}");
    }

    #[test]
    fn rfc8785_sort_example() {
        // Property names from RFC 8785 section 3.2.3.
        let v = json!({
            "\u{20ac}": "Euro Sign", "\r": "Carriage Return", "\u{fb33}": "Hebrew Letter Dalet With Dagesh",
            "1": "One", "\u{1f600}": "Emoji: Grinning Face", "\u{0080}": "Control", "\u{00f6}": "Latin Small Letter O With Diaeresis"
        });
        let order: Vec<String> = {
            let c = canonical(&v);
            let parsed: Vec<(String, Value)> = serde_json::from_str::<Map<String, Value>>(&c)
                .unwrap()
                .into_iter()
                .collect();
            // serde_json Map reorders; recover order from the string instead.
            let _ = parsed;
            let mut keys = vec![];
            let mut rest = c.as_str();
            while let Some(i) = rest.find("\":\"") {
                let start = rest[..i].rfind(['{', ',']).unwrap() + 2;
                keys.push(rest[start..i].to_string());
                rest = &rest[i + 3..];
            }
            keys
        };
        assert_eq!(order, vec!["\\r", "1", "\u{0080}", "\u{00f6}", "\u{20ac}", "\u{1f600}", "\u{fb33}"]);
    }

    #[test]
    fn log_links_prev_and_signs() {
        let mut l = Log::new("alice");
        let a = l.profile("Alice", "buyer");
        let b = l.profile("Alice B", "buyer");
        assert_eq!(b.header["prev"], json!(a.id));
        assert_eq!(a.header["prev"], Value::Null);
        assert_eq!(a.id.len(), 64);
        assert_eq!(a.sig.len(), 128);
    }
}
