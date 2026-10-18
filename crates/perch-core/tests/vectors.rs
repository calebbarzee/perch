//! Fixed test vectors in `docs/test-vectors.json`.
//!
//! - `vectors_match_file` (always runs) checks every vector against this
//!   implementation.
//! - `write_vectors` (ignored) regenerates the file:
//!   `cargo test -p perch-core --test vectors -- --ignored write_vectors`.

use perch_core::canonical::to_canonical_json;
use perch_core::ids::encode_hex;
use perch_core::serde_json::{self, Map, Value, json};
use perch_core::{
    Keypair, ProductKey, Record, RecordError, RecordId, Unsigned, check_record, sha256, sign_receipt, sign_record,
    verify_receipt,
};

const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/test-vectors.json");

fn seed(n: u8) -> [u8; 32] {
    // Seeds are SHA-256("perch test seed <n>") so they look random but are fixed.
    sha256(format!("perch test seed {n}").as_bytes())
}

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

struct Built {
    records: Vec<(String, String, Record, u8)>,
    receipts: Vec<Value>,
    invalid: Vec<Value>,
    canonical: Vec<Value>,
    product_keys: Vec<Value>,
}

fn build() -> Built {
    let seller = Keypair::from_seed(seed(1));
    let buyer = Keypair::from_seed(seed(2));
    let mut records = Vec::new();
    let mut push = |name: &str, note: &str, kp: &Keypair, k: u8, u: Unsigned| {
        let r = sign_record(kp, u).unwrap();
        records.push((name.to_owned(), note.to_owned(), r.clone(), k));
        r
    };

    let profile = push(
        "profile_seq1",
        "First record of a log: seq 1, prev null.",
        &seller,
        1,
        Unsigned {
            seq: 1,
            prev: None,
            ts: 1_760_000_000_000,
            kind: "profile".into(),
            replaces: None,
            body: obj(json!({"name": "Ada's Kettles", "role": "seller", "url": "https://kettles.example/"})),
        },
    );
    let product = push(
        "product_non_ascii",
        "Non-ASCII text (German, Japanese, emoji) is written as raw UTF-8, not escaped.",
        &seller,
        1,
        Unsigned {
            seq: 2,
            prev: Some(profile.id()),
            ts: 1_760_000_001_000,
            kind: "product".into(),
            replaces: None,
            body: obj(json!({
                "name": "Wasserkocher „Größe M“ 電気ケトル 😀",
                "brand": "Acme",
                "gtin": "4006381333931",
                "category": "küche/wasserkocher",
                "attributes": {"volumen": "1,7 l", "farbe": "rot"}
            })),
        },
    );
    let listing = push(
        "listing_seq3",
        "Listing referring to the product by its gtin: key.",
        &seller,
        1,
        Unsigned {
            seq: 3,
            prev: Some(product.id()),
            ts: 1_760_000_002_000,
            kind: "listing".into(),
            replaces: None,
            body: obj(json!({
                "product": "gtin:04006381333931",
                "url": "https://kettles.example/p/m",
                "price": {"amount": 3999, "currency": "EUR"},
                "availability": "in_stock",
                "condition": "new",
                "ships_to": ["AT", "DE"]
            })),
        },
    );
    let _listing2 = push(
        "listing_replaces",
        "New version of the listing: carries replaces; price changed.",
        &seller,
        1,
        Unsigned {
            seq: 4,
            prev: Some(listing.id()),
            ts: 1_760_000_003_000,
            kind: "listing".into(),
            replaces: Some(listing.id()),
            body: obj(json!({
                "product": "gtin:04006381333931",
                "url": "https://kettles.example/p/m",
                "price": {"amount": 3499, "currency": "EUR"},
                "availability": "in_stock",
                "condition": "new"
            })),
        },
    );
    let receipt = sign_receipt(&seller, listing.id(), buyer.author(), 1_760_000_100_000, seed(99)).unwrap();
    let review = push(
        "review_escapes_receipt",
        "String escapes (quote, backslash, newline, tab, U+0001, U+001F) and a nested receipt object.",
        &buyer,
        2,
        Unsigned {
            seq: 1,
            prev: None,
            ts: 1_760_000_200_000,
            kind: "review".into(),
            replaces: None,
            body: obj(json!({
                "product": "gtin:04006381333931",
                "rating": 4,
                "title": "Quote \" and backslash \\",
                "text": "Line 1\nLine 2\tTabbed \u{1} control \u{1f} end / slash",
                "receipt": receipt.to_value()
            })),
        },
    );
    let flagged = push(
        "unknown_kind_header_only_source",
        "Unknown kind with unknown nested fields; a valid record that views ignore. Also the source of the header-only vector.",
        &buyer,
        2,
        Unsigned {
            seq: 2,
            prev: Some(review.id()),
            ts: 1_760_000_300_000,
            kind: "x.experiment_1".into(),
            replaces: None,
            body: obj(json!({"z": [], "a": {"nested": [1, -2, true, false, null]}, "\u{e000}": 1, "\u{1f600}": 2})),
        },
    );
    records.push((
        "header_only".into(),
        "The previous record with its body removed (redacted). Same id and sig; no body field.".into(),
        flagged.without_body(),
        2,
    ));

    let receipts = vec![json!({
        "name": "receipt_1",
        "note": "Detached receipt signed by the seller over the receipt domain. Cited by review_escapes_receipt.",
        "seller_seed": encode_hex(&seed(1)),
        "seller": seller.author().to_hex(),
        "receipt": receipt.to_value(),
        "signed_canonical": receipt.signed_canonical(),
        "signed_message_hex": encode_hex(&[b"perch/receipt/v1\n".as_slice(), receipt.signed_canonical().as_bytes()].concat()),
    })];

    // Invalid inputs: each must be rejected at the isolated level.
    let good = profile.to_json_string();
    let first_char_of_sig = good.find("\"sig\":\"").unwrap() + 7;
    let mut flipped = good.clone().into_bytes();
    flipped[first_char_of_sig] = if flipped[first_char_of_sig] == b'0' { b'1' } else { b'0' };
    let flipped = String::from_utf8(flipped).unwrap();
    let bs = '\\';
    let invalid_cases: Vec<(&str, String, &str)> = vec![
        ("tampered_sig", flipped, "signature"),
        ("tampered_ts", good.replace("\"ts\":1760000000000", "\"ts\":1760000000001"), "signature"),
        ("uppercase_hex", good.replace(&profile.author().to_hex(), &profile.author().to_hex().to_uppercase()), "invalid_field"),
        ("duplicate_key", good.replacen("{\"author\"", "{\"kind\":\"profile\",\"author\"", 1), "json"),
        ("float_token", good.replace("\"v\":1", "\"v\":1.0"), "json"),
        ("exponent_token", good.replace("\"v\":1", "\"v\":1e0"), "json"),
        ("negative_zero", good.replace("\"name\":\"Ada's Kettles\"", "\"name\":\"Ada's Kettles\",\"n\":-0"), "json"),
        ("lone_surrogate", good.replace("Ada's Kettles", &format!("Ada{bs}ud800")), "json"),
        ("replaces_null", good.replace(",\"seq\":1", ",\"replaces\":null,\"seq\":1"), "invalid_field"),
        ("unknown_field", good.replace(",\"v\":1}", ",\"v\":1,\"extra\":0}"), "unknown_field"),
        ("body_hash_mismatch", good.replace("Ada's Kettles", "Bob's Kettles"), "body_hash_mismatch"),
        ("prev_not_null_at_seq1", good.replace("\"prev\":null", &format!("\"prev\":\"{}\"", "0".repeat(64))), "prev_mismatch"),
    ];
    let invalid = invalid_cases
        .into_iter()
        .map(|(name, line, error)| {
            let got = Record::from_json_str(&line).expect_err(name);
            let got_tag = serde_json::to_value(&got).unwrap()["error"].as_str().unwrap().to_owned();
            assert_eq!(got_tag, error, "{name}: {got:?}");
            json!({"name": name, "wire": line, "error": error})
        })
        .collect();

    let canonical = [
        json!({"b": 1, "a": 2}),
        json!({"\u{20ac}": "Euro Sign", "\r": "Carriage Return", "\u{fb33}": "Hebrew Letter Dalet With Dagesh", "1": "One", "\u{1f600}": "Emoji: Grinning Face", "\u{80}": "Control", "\u{f6}": "Latin Small Letter O With Diaeresis"}),
        json!(["\u{0}\u{8}\u{9}\u{a}\u{c}\u{d}\u{1f}\"\\/\u{7f}\u{2028}"]),
        json!({"n": [0, -1, 9007199254740991u64, -9007199254740991i64]}),
    ]
    .into_iter()
    .map(|v| json!({"value": v.clone(), "canonical": to_canonical_json(&v).unwrap()}))
    .collect();

    let root = RecordId::from_bytes(seed(7));
    let product_keys = [
        (json!({"brand": "Acme", "gtin": "96385074"}), ProductKey::derive(Some("96385074"), "Acme", None, root)),
        (json!({"brand": "Acme", "gtin": "036000291452", "mpn": "K-1"}), ProductKey::derive(Some("036000291452"), "Acme", Some("K-1"), root)),
        (json!({"brand": "  ACME Corp\t", "mpn": " WH-1000XM5 "}), ProductKey::derive(None, "  ACME Corp\t", Some(" WH-1000XM5 "), root)),
        (json!({"brand": "MÜLLER", "mpn": "Ä-1"}), ProductKey::derive(None, "MÜLLER", Some("Ä-1"), root)),
        (json!({"brand": "Acme"}), ProductKey::derive(None, "Acme", None, root)),
    ]
    .into_iter()
    .map(|(fields, key)| json!({"fields": fields, "root": root, "key": key.unwrap()}))
    .collect();

    Built { records, receipts, invalid, canonical, product_keys }
}

fn record_vector(name: &str, note: &str, r: &Record, k: u8) -> Value {
    let mut v = json!({
        "name": name,
        "note": note,
        "seed": encode_hex(&seed(k)),
        "author": r.author().to_hex(),
        "header_canonical": r.header_canonical(),
        "signed_message_hex": encode_hex(&[b"perch/record/v1\n".as_slice(), r.header_canonical().as_bytes()].concat()),
        "id": r.id().to_hex(),
        "sig": r.sig().to_hex(),
        "body_hash": r.body_hash().to_hex(),
        "wire": r.to_json_string(),
    });
    if let Some(b) = r.body_canonical() {
        v["body_canonical"] = json!(b);
    }
    v
}

fn document() -> Value {
    let b = build();
    json!({
        "description": "Perch record format test vectors (ARCHITECTURE contract version 3). See docs/data-model.md.",
        "seeds": "Each seed is SHA-256 of the ASCII string \"perch test seed <n>\". The receipt nonce is the seed for n = 99. These keys are public; never use them for real data.",
        "domains": {"record": "perch/record/v1\n", "receipt": "perch/receipt/v1\n"},
        "records": b.records.iter().map(|(n, note, r, k)| record_vector(n, note, r, *k)).collect::<Vec<_>>(),
        "receipts": b.receipts,
        "invalid": b.invalid,
        "canonical_json": b.canonical,
        "product_keys": b.product_keys,
    })
}

fn render(doc: &Value) -> String {
    let mut s = serde_json::to_string_pretty(doc).unwrap();
    s.push('\n');
    s
}

#[test]
#[ignore = "writes docs/test-vectors.json"]
fn write_vectors() {
    std::fs::write(PATH, render(&document())).unwrap();
}

#[test]
fn vectors_match_file() {
    let text = std::fs::read_to_string(PATH).expect("docs/test-vectors.json exists; run write_vectors");
    let file: Value = serde_json::from_str(&text).unwrap();
    // The file is reproducible from fixed seeds.
    assert_eq!(file, document(), "docs/test-vectors.json is out of date; run write_vectors");

    // Check each vector independently of how it was generated.
    for v in file["records"].as_array().unwrap() {
        let name = v["name"].as_str().unwrap();
        let r = Record::from_json_str(v["wire"].as_str().unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let kp = Keypair::from_seed_hex(v["seed"].as_str().unwrap()).unwrap();
        assert_eq!(kp.author().to_hex(), v["author"], "{name}");
        assert_eq!(r.author(), kp.author());
        assert_eq!(r.header_canonical(), v["header_canonical"], "{name}");
        assert_eq!(encode_hex(&sha256(r.header_canonical().as_bytes())), v["id"], "{name}");
        assert_eq!(r.id().to_hex(), v["id"]);
        assert_eq!(r.sig().to_hex(), v["sig"]);
        assert_eq!(r.body_hash().to_hex(), v["body_hash"]);
        assert_eq!(r.to_json_string(), v["wire"]);
        match v.get("body_canonical") {
            Some(b) => {
                let b = b.as_str().unwrap();
                assert_eq!(encode_hex(&sha256(b.as_bytes())), v["body_hash"], "{name}");
                assert_eq!(r.body_canonical(), Some(b));
            }
            None => assert!(!r.has_body(), "{name}"),
        }
        // Ed25519 is deterministic: re-signing gives the same signature.
        let msg = [b"perch/record/v1\n".as_slice(), r.header_canonical().as_bytes()].concat();
        assert_eq!(encode_hex(&msg), v["signed_message_hex"]);
        assert_eq!(kp.sign_raw(&msg), r.sig());
        if r.has_body() && r.kind() != "x.experiment_1" {
            check_record(&r).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        }
    }
    let n_records = file["records"].as_array().unwrap().len();
    assert!(n_records >= 5);

    for v in file["receipts"].as_array().unwrap() {
        let rc = perch_core::receipt::parse_receipt(&v["receipt"]).unwrap();
        let seller = perch_core::AuthorId::from_hex(v["seller"].as_str().unwrap()).unwrap();
        verify_receipt(&rc, &seller).unwrap();
        assert_eq!(rc.signed_canonical(), v["signed_canonical"]);
    }

    for v in file["invalid"].as_array().unwrap() {
        let e: RecordError = Record::from_json_str(v["wire"].as_str().unwrap()).unwrap_err();
        assert_eq!(serde_json::to_value(&e).unwrap()["error"], v["error"], "{}", v["name"]);
    }

    for v in file["canonical_json"].as_array().unwrap() {
        assert_eq!(to_canonical_json(&v["value"]).unwrap(), v["canonical"]);
    }

    for v in file["product_keys"].as_array().unwrap() {
        let f = &v["fields"];
        let root = RecordId::from_hex(v["root"].as_str().unwrap()).unwrap();
        let k = ProductKey::derive(f["gtin"].as_str(), f["brand"].as_str().unwrap(), f["mpn"].as_str(), root).unwrap();
        assert_eq!(k.as_str(), v["key"]);
    }
}
