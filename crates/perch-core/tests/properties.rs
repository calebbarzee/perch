//! Property tests for canonical encoding and record integrity.

use perch_core::canonical::to_canonical_json;
use perch_core::json;
use perch_core::serde_json::{Map, Number, Value};
use perch_core::{Keypair, Record, RecordId, Unsigned, sign_record};
use proptest::prelude::*;

const MAX: i64 = (1 << 53) - 1;

fn any_string() -> impl Strategy<Value = String> {
    // Mix ASCII, control characters, escapes, BMP and astral characters.
    let ch = prop_oneof![
        4 => proptest::char::range('a', 'z'),
        2 => proptest::char::range('\u{0}', '\u{7f}'),
        1 => Just('"'),
        1 => Just('\\'),
        1 => proptest::char::range('\u{80}', '\u{7ff}'),
        1 => proptest::char::range('\u{e000}', '\u{ffff}'),
        1 => proptest::char::range('\u{10000}', '\u{10ffff}'),
        1 => any::<char>(),
    ];
    proptest::collection::vec(ch, 0..12).prop_map(|v| v.into_iter().collect())
}

fn any_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        (-MAX..=MAX).prop_map(|n| Value::Number(Number::from(n))),
        any_string().prop_map(Value::String),
    ];
    leaf.prop_recursive(5, 48, 6, |inner| {
        prop_oneof![
            proptest::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            proptest::collection::vec((any_string(), inner), 0..6)
                .prop_map(|kv| Value::Object(kv.into_iter().collect::<Map<_, _>>())),
        ]
    })
}

fn any_object() -> impl Strategy<Value = Map<String, Value>> {
    proptest::collection::vec((any_string(), any_json()), 0..6).prop_map(|kv| kv.into_iter().collect())
}

/// Serializes `v` with object keys in an order chosen by `perm`, random
/// whitespace, and optional `\u` escapes. Any strict JSON parser must read
/// it back as the same value.
fn noisy(v: &Value, rng: &mut impl FnMut() -> u32, out: &mut String) {
    let ws = |rng: &mut dyn FnMut() -> u32, out: &mut String| {
        for _ in 0..(rng() % 3) {
            out.push([' ', '\n', '\t', '\r'][(rng() % 4) as usize]);
        }
    };
    ws(rng, out);
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            // Fisher-Yates shuffle with the provided generator.
            for i in (1..keys.len()).rev() {
                let j = (rng() as usize) % (i + 1);
                keys.swap(i, j);
            }
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                noisy_str(k, rng, out);
                ws(rng, out);
                out.push(':');
                noisy(&m[*k], rng, out);
            }
            ws(rng, out);
            out.push('}');
        }
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                noisy(x, rng, out);
            }
            ws(rng, out);
            out.push(']');
        }
        Value::String(s) => noisy_str(s, rng, out),
        other => out.push_str(&other.to_string()),
    }
    ws(rng, out);
}

fn noisy_str(s: &str, rng: &mut impl FnMut() -> u32, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        let must = c == '"' || c == '\\' || (c as u32) < 0x20;
        if must || rng() % 4 == 0 {
            match (c, rng() % 2) {
                ('/', 0) => out.push_str("\\/"),
                ('"', 0) => out.push_str("\\\""),
                ('\\', 0) => out.push_str("\\\\"),
                ('\n', 0) => out.push_str("\\n"),
                _ => {
                    let mut buf = [0u16; 2];
                    for u in c.encode_utf16(&mut buf) {
                        if rng() % 2 == 0 {
                            out.push_str(&format!("\\u{:04x}", u));
                        } else {
                            out.push_str(&format!("\\u{:04X}", u));
                        }
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out.push('"');
}

fn lcg(seed: u64) -> impl FnMut() -> u32 {
    let mut s = seed | 1;
    move || {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (s >> 33) as u32
    }
}

fn record_strategy() -> impl Strategy<Value = Record> {
    (
        any::<[u8; 32]>(),
        1u64..1000,
        0u64..(1 << 53),
        prop::sample::select(vec!["profile", "review", "x", "attest.identity", "a.b_c"]),
        any::<bool>(),
        any::<[u8; 32]>(),
        any_object(),
    )
        .prop_map(|(seed, seq, ts, kind, has_replaces, rid, body)| {
            let kp = Keypair::from_seed(seed);
            let prev = if seq == 1 { None } else { Some(RecordId(rid)) };
            let replaces = has_replaces.then_some(RecordId(rid));
            sign_record(&kp, Unsigned { seq, prev, ts, kind: kind.into(), replaces, body }).unwrap()
        })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    /// encode(parse(encode(x))) == encode(x), and parse(encode(x)) == x.
    #[test]
    fn canonical_is_stable(v in any_json()) {
        let c = to_canonical_json(&v).unwrap();
        let back = json::parse(&c, 64).unwrap();
        prop_assert_eq!(&back, &v);
        prop_assert_eq!(to_canonical_json(&back).unwrap(), c);
    }

    /// Key order, whitespace, and optional escapes in the input do not
    /// change the encoding.
    #[test]
    fn canonical_ignores_input_form(v in any_json(), seed in any::<u64>()) {
        let mut text = String::new();
        noisy(&v, &mut lcg(seed), &mut text);
        let parsed = json::parse(&text, 64).unwrap();
        prop_assert_eq!(to_canonical_json(&parsed).unwrap(), to_canonical_json(&v).unwrap());
    }

    /// The wire form is canonical and round-trips; the header-only form
    /// keeps the ID and still verifies.
    #[test]
    fn wire_roundtrip(r in record_strategy()) {
        let w = r.to_json_string();
        prop_assert!(perch_core::canonical::is_canonical(&w));
        let back = Record::from_json_str(&w).unwrap();
        prop_assert_eq!(back.to_json_string(), w);
        let h = Record::from_json_str(&r.without_body().to_json_string()).unwrap();
        prop_assert_eq!(h.id(), r.id());
        prop_assert!(!h.has_body());
    }

    /// Any single-byte change to the wire form fails to parse, fails
    /// verification, or yields a different ID.
    #[test]
    fn single_byte_mutation_detected(r in record_strategy(), pos in any::<prop::sample::Index>(), byte in any::<u8>(), header_only in any::<bool>()) {
        let r = if header_only { r.without_body() } else { r };
        let w = r.to_json_string();
        let mut bytes = w.clone().into_bytes();
        let i = pos.index(bytes.len());
        prop_assume!(bytes[i] != byte);
        bytes[i] = byte;
        let Ok(text) = String::from_utf8(bytes) else {
            return Ok(()); // not UTF-8: fails to parse
        };
        if let Ok(m) = Record::from_json_str(&text) {
            prop_assert_ne!(m.id(), r.id(), "mutation at {} kept the id: {}", i, text);
        }
    }
}

#[test]
fn mutation_exhaustive_on_one_record() {
    // Every position, a handful of replacement bytes, on one fixed record.
    let kp = Keypair::from_seed([9; 32]);
    let mut body = Map::new();
    body.insert("text".into(), Value::String("a/\"\\\n é 😀".into()));
    body.insert("n".into(), Value::from(-42));
    let r = sign_record(&kp, Unsigned { seq: 2, prev: Some(RecordId([1; 32])), ts: 5, kind: "review".into(), replaces: Some(RecordId([2; 32])), body }).unwrap();
    let w = r.to_json_string().into_bytes();
    let mut accepted = 0;
    for i in 0..w.len() {
        for b in [b' ', b'0', b'1', b'a', b'A', b'"', b'\\', b'{', b',', b'-', 0x7f] {
            if w[i] == b {
                continue;
            }
            let mut m = w.clone();
            m[i] = b;
            if let Ok(t) = String::from_utf8(m)
                && let Ok(rec) = Record::from_json_str(&t)
            {
                assert_ne!(rec.id(), r.id());
                accepted += 1;
            }
        }
    }
    assert_eq!(accepted, 0, "a mutated record verified");
}
