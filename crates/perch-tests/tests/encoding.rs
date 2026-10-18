//! Canonical encoding (contract 4.2): equivalent inputs verify and normalize
//! to one form; invalid inputs fail.

use perch_tests::fixture::{Log, canonical};
use perch_tests::require_sut;
use perch_tests::sut::{normalize, verify};
use perch_tests::vectors::{equivalent_encodings, invalid_encodings};
use serde_json::json;

#[test]
fn key_order_and_whitespace_verify() {
    require_sut!(Core);
    let mut l = Log::new("enc-alice");
    let r = l.add(
        "profile",
        json!({ "name": "Alice", "role": "buyer", "about": "Line one\nline \"two\" \\ tab\t\u{1} end" }),
    );
    for (label, enc) in equivalent_encodings(&r) {
        assert_eq!(verify(&enc), Ok(r.id.clone()), "equivalent encoding '{label}' must verify");
        assert_eq!(normalize(&enc), Ok(r.wire()), "'{label}' must normalize to the canonical form");
    }
}

#[test]
fn escaped_equivalent_strings_verify() {
    require_sut!(Core);
    let mut l = Log::new("enc-esc");
    let r = l.add("profile", json!({ "name": "Zoë/€", "role": "buyer" }));
    let w = r.wire();
    // \u-escapes for non-ASCII, and \/ for /, are valid JSON input with the same value.
    let escaped = w.replacen("Zoë/€", "Zo\\u00ebu\\/\\u20ac", 1).replacen("u\\/", "\\/", 1);
    assert_eq!(verify(&escaped), Ok(r.id.clone()));
    assert_eq!(normalize(&escaped), Ok(w));
}

#[test]
fn keys_sorted_by_utf16_not_utf8() {
    require_sut!(Core);
    // Body keys U+FF61 and U+1F600 sort differently in UTF-8 and UTF-16.
    let mut l = Log::new("enc-utf16");
    let r = l.add("profile", json!({ "name": "U", "role": "other", "\u{ff61}": 1, "\u{1f600}": 2 }));
    let wire = r.wire();
    assert!(wire.find('\u{1f600}').unwrap() < wire.find('\u{ff61}').unwrap(), "fixture uses UTF-16 order");
    assert_eq!(normalize(&wire), Ok(wire.clone()), "implementation must keep UTF-16 order");
    assert_eq!(verify(&wire), Ok(r.id.clone()));
}

#[test]
fn invalid_encodings_rejected() {
    require_sut!(Core);
    let mut l = Log::new("enc-bad");
    for (label, enc) in invalid_encodings(&mut l) {
        assert!(verify(&enc).is_err(), "invalid encoding '{label}' must be rejected: {enc}");
    }
}

#[test]
fn duplicate_key_rejected() {
    require_sut!(Core);
    let mut l = Log::new("enc-dup");
    let r = l.add("profile", json!({ "name": "A", "role": "buyer" }));
    let w = r.wire().replacen("\"name\":\"A\"", "\"name\":\"A\",\"name\":\"A\"", 1);
    assert!(verify(&w).is_err(), "duplicate keys with equal values are still invalid");
}

#[test]
fn non_integer_number_tokens_rejected() {
    require_sut!(Core);
    let mut l = Log::new("enc-num");
    let r = l.add("review", json!({ "product": "gtin:04006381333931", "rating": 4, "text": "t" }));
    for bad in ["4.0", "4e0", "4E0", "0.4e1"] {
        let w = r.wire().replacen("\"rating\":4", &format!("\"rating\":{bad}"), 1);
        assert!(verify(&w).is_err(), "number token {bad} must be rejected");
    }
}

#[test]
fn lone_surrogate_rejected() {
    require_sut!(Core);
    let mut l = Log::new("enc-sur");
    let r = l.add("profile", json!({ "name": "A", "role": "buyer" }));
    for bad in ["\\ud800", "\\udfff", "\\udc00\\ud800"] {
        let w = r.wire().replacen("\"name\":\"A\"", &format!("\"name\":\"A{bad}\""), 1);
        assert!(verify(&w).is_err(), "{bad} must be rejected");
    }
}

#[test]
fn uppercase_hex_rejected() {
    require_sut!(Core);
    let mut l = Log::new("enc-hex");
    l.profile("A", "buyer");
    let r = l.profile("B", "buyer");
    let prev = r.header["prev"].as_str().unwrap().to_string();
    assert!(verify(&r.wire().replace(&prev, &prev.to_uppercase())).is_err());
    assert!(verify(&r.wire().replace(&r.sig, &r.sig.to_uppercase())).is_err());
}

#[test]
fn replaces_null_rejected() {
    require_sut!(Core);
    let mut l = Log::new("enc-repnull");
    let r = l.profile("A", "buyer");
    let w = r.wire_edited(|m| {
        m.insert("replaces".into(), serde_json::Value::Null);
    });
    assert!(verify(&w).is_err());
}

#[test]
fn unknown_top_level_field_rejected() {
    require_sut!(Core);
    let mut l = Log::new("enc-unk");
    let r = l.profile("A", "buyer");
    let w = r.wire_edited(|m| {
        m.insert("extra".into(), json!("x"));
    });
    assert!(verify(&w).is_err());
}

#[test]
fn unknown_body_fields_preserved() {
    require_sut!(Core);
    let mut l = Log::new("enc-future");
    let r = l.add("profile", json!({ "name": "A", "role": "buyer", "future_field": { "z": 1, "a": [true, null] } }));
    assert_eq!(normalize(&r.wire()), Ok(r.wire()), "unknown body fields must survive normalization");
    assert!(r.wire().contains("future_field"));
    let _ = canonical(&json!(null));
}

#[test]
fn unknown_kind_passes_isolated_level() {
    require_sut!(Core);
    let mut l = Log::new("enc-kind");
    let r = l.add("future.kind_v2", json!({ "anything": "goes" }));
    assert_eq!(verify(&r.wire()), Ok(r.id.clone()));
}
