//! Size and shape limits (contract 4.2, 9).
//!
//! Every rejected vector is otherwise valid: its signature and `body_hash`
//! match, so it fails only because of the limit under test.

use perch_tests::fixture::{Key, Log, canonical, canonical_unchecked, header1, sign_raw_body};
use perch_tests::require_sut;
use perch_tests::sut::verify;
use perch_tests::vectors::{body_of_len, nested_body, wide_body};
use serde_json::json;

#[test]
fn body_at_limit_accepted() {
    require_sut!(Core);
    let mut l = Log::new("lim-max");
    let r = l.add("profile", body_of_len(65_536));
    assert_eq!(canonical(r.body.as_ref().unwrap()).len(), 65_536);
    assert_eq!(verify(&r.wire()), Ok(r.id.clone()));
}

#[test]
fn oversize_body_rejected() {
    require_sut!(Core);
    let mut l = Log::new("lim-over");
    let r = l.add("profile", body_of_len(65_537));
    assert!(r.wire().len() < 131_072, "line limit is not the reason");
    assert!(verify(&r.wire()).is_err());
}

#[test]
fn oversize_line_rejected() {
    require_sut!(Core);
    // Whitespace outside strings is valid JSON; normalized, the record is small.
    let mut l = Log::new("lim-line");
    let r = l.profile("A", "buyer");
    let at_limit = r.wire().replacen("{", &format!("{{{}", " ".repeat(131_072 - r.wire().len())), 1);
    assert_eq!(at_limit.len(), 131_072);
    assert_eq!(verify(&at_limit), Ok(r.id.clone()), "exactly 131,072 bytes is allowed");
    let over = r.wire().replacen("{", &format!("{{{}", " ".repeat(131_073 - r.wire().len())), 1);
    assert_eq!(over.len(), 131_073);
    assert!(verify(&over).is_err(), "131,073 bytes is rejected before parsing");
}

#[test]
fn depth_16_accepted_depth_17_rejected() {
    require_sut!(Core);
    // Depth counts from the record object (1); body is 2. Both vectors are signed.
    let mut l = Log::new("lim-depth");
    let ok = l.add("profile", nested_body(16));
    let key = Key::from_label("lim-depth17");
    let deep17 = canonical_unchecked(&nested_body(17));
    let (bad, _) = sign_raw_body(&key, header1(&key, "profile"), &deep17);
    assert_eq!(verify(&ok.wire()), Ok(ok.id.clone()), "depth 16 is allowed");
    assert!(verify(&bad).is_err(), "depth 17 is invalid");
}

#[test]
fn very_deep_json_does_not_crash() {
    require_sut!(Core);
    // 100,000 nested arrays inside a correctly hashed and signed body.
    let key = Key::from_label("lim-deep");
    let deep = format!("{{\"name\":\"A\",\"role\":\"buyer\",\"x\":{}0{}}}", "[".repeat(60_000), "]".repeat(60_000));
    let (line, _) = sign_raw_body(&key, header1(&key, "profile"), &deep);
    assert!(line.len() <= 131_072, "line limit is not the reason");
    assert!(verify(&line).is_err());
}

#[test]
fn wide_body_within_limit_accepted() {
    require_sut!(Core);
    let mut l = Log::new("lim-wide");
    let r = l.add("profile", wide_body(60_000));
    assert!(r.body.as_ref().unwrap().as_object().unwrap().len() > 4_000);
    assert_eq!(verify(&r.wire()), Ok(r.id.clone()));
}

#[test]
fn integer_beyond_2_53_rejected() {
    require_sut!(Core);
    let key = Key::from_label("lim-int");
    for n in ["9007199254740992", "-9007199254740992", "18446744073709551616"] {
        let body = format!("{{\"n\":{n},\"name\":\"A\",\"role\":\"buyer\"}}");
        let (line, _) = sign_raw_body(&key, header1(&key, "profile"), &body);
        assert!(verify(&line).is_err(), "{n} is out of range");
    }
    for n in ["9007199254740991", "-9007199254740991"] {
        let body = format!("{{\"n\":{n},\"name\":\"A\",\"role\":\"buyer\"}}");
        let (line, id) = sign_raw_body(&key, header1(&key, "profile"), &body);
        assert_eq!(verify(&line), Ok(id), "{n} is in range");
    }
    let _ = json!(0);
}

#[test]
fn ts_beyond_2_53_rejected() {
    require_sut!(Core);
    let key = Key::from_label("lim-ts");
    let mut h = header1(&key, "profile");
    h.insert("ts".into(), json!(9_007_199_254_740_992u64));
    let (line, _) = sign_raw_body_unchecked_header(&key, h, "{\"name\":\"A\",\"role\":\"buyer\"}");
    assert!(verify(&line).is_err());
}

/// Like `sign_raw_body`, but the header may hold out-of-range integers.
fn sign_raw_body_unchecked_header(
    key: &Key,
    mut header: serde_json::Map<String, serde_json::Value>,
    body_raw: &str,
) -> (String, String) {
    use perch_tests::fixture::{RECORD_DOMAIN, sha256_hex};
    header.insert("body_hash".into(), json!(sha256_hex(body_raw.as_bytes())));
    let hc = canonical_unchecked(&serde_json::Value::Object(header.clone()));
    let sig = key.sign_hex(RECORD_DOMAIN, hc.as_bytes());
    let mut m = header;
    m.insert("sig".into(), json!(sig));
    let without = canonical_unchecked(&serde_json::Value::Object(m));
    let at = without.find(",\"body_hash\"").unwrap();
    (format!("{},\"body\":{}{}", &without[..at], body_raw, &without[at..]), sha256_hex(hc.as_bytes()))
}
