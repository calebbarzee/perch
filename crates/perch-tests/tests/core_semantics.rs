//! Core semantic helpers that views depend on (contract 4.5, 4.8, 5.2).
//! These call perch-core directly, because they are pure functions of input.

use perch_core::{
    EvidencePair, ProductKey, Record, check_record, is_control_kind, is_replaceable,
    weburl::{host_matches_domain, https_url_host},
};
use perch_tests::fixture::{Log, Opts};
use perch_tests::require_sut;
use serde_json::json;

fn rec(line: &str) -> Record {
    Record::from_json_str(line).unwrap()
}

#[test]
fn url_rules_reject_phishing_forms() {
    require_sut!(Core);
    let bad = [
        "http://shop.example/",                 // not https
        "https://shop.example@evil.example/",   // userinfo
        "https://user:pw@shop.example/",        // userinfo
        "https://192.168.1.1/",                 // IPv4 host
        "https://[::1]/",                       // IPv6 host
        "https://3232235777/",                  // IPv4 as one integer (WHATWG parses it)
        "https://0x7f.1/",                      // hex IPv4 form
        "javascript:alert(1)",
        "data:text/html,x",
        "https://",
        "",
    ];
    for u in bad {
        assert!(https_url_host(u).is_err(), "{u:?} must be rejected");
    }
    let long = format!("https://shop.example/{}", "a".repeat(2048));
    assert!(https_url_host(&long).is_err(), "over 2048 characters");
}

#[test]
fn url_hosts_are_ascii_and_lowercase() {
    require_sut!(Core);
    assert_eq!(https_url_host("https://WWW.Shop.Example/x").unwrap(), "www.shop.example");
    // Cyrillic "а" (U+0430) look-alike becomes punycode, so it cannot equal "shop.example".
    let h = https_url_host("https://sh\u{043e}p.example/").unwrap();
    assert!(h.starts_with("xn--"), "IDNA ToASCII expected, got {h}");
    assert_ne!(h, "shop.example");
    // Percent-encoded and soft-hyphen forms normalize to the plain host (WHATWG),
    // so the displayed host is the host the browser will contact.
    assert_eq!(https_url_host("https://ex%61mple.com/").unwrap(), "example.com");
    assert_eq!(https_url_host("https://exa\u{00ad}mple.com/").unwrap(), "example.com");
    // A trailing-dot host never matches the undotted attested domain (fails closed).
    let t = https_url_host("https://shop.example./").unwrap();
    assert!(!host_matches_domain(&t, "shop.example"));
}

#[test]
fn domain_matching_is_label_aligned() {
    require_sut!(Core);
    assert!(host_matches_domain("shop.example", "shop.example"));
    assert!(host_matches_domain("www.shop.example", "shop.example"));
    assert!(!host_matches_domain("xshop.example", "shop.example"));
    assert!(!host_matches_domain("shop.example.evil.example", "shop.example"));
    assert!(!host_matches_domain("example", "shop.example"));
}

#[test]
fn control_and_replaceable_kinds_match_contract() {
    require_sut!(Core);
    for k in ["attest.identity", "attest.domain", "attest.revoke", "retract", "retire"] {
        assert!(is_control_kind(k), "{k} is a control kind (4.5)");
    }
    for k in ["profile", "product", "listing", "review", "flag", "appeal", "observation", "redact"] {
        assert!(!is_control_kind(k), "{k} is not a control kind");
    }
    for k in ["profile", "attest.identity", "attest.domain", "product", "listing", "review", "flag", "appeal"] {
        assert!(is_replaceable(k), "{k} is replaceable");
    }
    for k in ["attest.revoke", "observation", "retract", "retire", "redact"] {
        assert!(!is_replaceable(k), "{k} is not replaceable");
    }
}

#[test]
fn non_replaceable_kind_with_replaces_is_semantically_invalid() {
    require_sut!(Core);
    let mut l = Log::new("cs-nonrep");
    let f = l.flag(&"a".repeat(64), "spam");
    let r = l.add_with("retract", json!({ "target": f.id }), Opts { replaces: Some(f.id.clone()), ..Default::default() });
    assert!(check_record(&rec(&r.wire())).is_err());
}

#[test]
fn body_schema_rejects_bad_values() {
    require_sut!(Core);
    let mut l = Log::new("cs-schema");
    let cases = [
        ("review", json!({ "product": "gtin:04006381333931", "rating": 6, "text": "x" })),
        ("review", json!({ "product": "gtin:04006381333931", "rating": 0, "text": "x" })),
        ("review", json!({ "product": "gtin:04006381333931", "rating": 3, "text": "" })),
        ("review", json!({ "product": "gtin:04006381333932", "rating": 3, "text": "x" })), // bad check digit
        ("listing", json!({ "product": "gtin:04006381333931", "url": "http://a.example/", "price": { "amount": 1, "currency": "EUR" }, "availability": "in_stock", "condition": "new" })),
        ("listing", json!({ "product": "gtin:04006381333931", "url": "https://a.example/", "price": { "amount": -1, "currency": "EUR" }, "availability": "in_stock", "condition": "new" })),
        ("listing", json!({ "product": "gtin:04006381333931", "url": "https://a.example/", "price": { "amount": 1, "currency": "XYZ" }, "availability": "in_stock", "condition": "new" })),
        ("flag", json!({ "target": "a".repeat(64), "reason": "dislike" })),
        ("attest.identity", json!({ "subject": "a".repeat(64), "person": "b".repeat(64), "level": "person", "legal_name": "Acme Ltd", "expires": 1 })),
        ("profile", json!({ "name": "", "role": "buyer" })),
        ("profile", json!({ "name": "x".repeat(101), "role": "buyer" })),
        ("profile", json!({ "name": "A", "role": "buyer", "url": "https://a.example@b.example/" })),
        ("product", json!({ "name": "W", "brand": "B", "images": [{ "url": "http://img.example/a.png" }] })),
    ];
    for (kind, body) in cases {
        let r = l.add(kind, body.clone());
        assert!(check_record(&rec(&r.wire())).is_err(), "{kind} {body} must fail the schema");
    }
}

#[test]
fn text_lengths_count_scalar_values_not_bytes() {
    require_sut!(Core);
    // 100 four-byte emoji = 100 scalar values = valid profile name (1–100).
    let mut l = Log::new("cs-len");
    let ok = l.add("profile", json!({ "name": "\u{1f600}".repeat(100), "role": "buyer" }));
    assert!(check_record(&rec(&ok.wire())).is_ok());
    let bad = l.add("profile", json!({ "name": "\u{1f600}".repeat(101), "role": "buyer" }));
    assert!(check_record(&rec(&bad.wire())).is_err());
}

#[test]
fn product_keys_match_contract() {
    require_sut!(Core);
    assert_eq!(ProductKey::from_gtin("4006381333931").unwrap().as_str(), "gtin:04006381333931");
    assert_eq!(ProductKey::from_gtin("04006381333931").unwrap().as_str(), "gtin:04006381333931");
    assert!(ProductKey::from_gtin("4006381333932").is_err(), "bad check digit");
    assert!(ProductKey::from_gtin("1234567").is_err(), "7 digits");
    assert_eq!(ProductKey::from_brand_mpn("  Acme ", " XY-12 ").unwrap().as_str(), "mpn:acme:xy-12");
    // Only ASCII letters are lowercased (4.8): non-ASCII stays as is.
    let k = ProductKey::from_brand_mpn("ÄCME", "X").unwrap();
    assert_eq!(k.as_str(), "mpn:Äcme:x");
}

#[test]
fn evidence_pair_order_is_total_and_canonical() {
    require_sut!(Core);
    let mut l = Log::new("cs-fork");
    l.profile("A", "buyer");
    l.profile("B", "buyer");
    let x = l.fork_at(2, "profile", json!({ "name": "X", "role": "buyer" }));
    let y = l.fork_at(2, "profile", json!({ "name": "Y", "role": "buyer" }));
    let honest = rec(&l.records[1].wire());
    let (x, y) = (rec(&x.wire()), rec(&y.wire()));
    let p1 = EvidencePair::new(honest.clone(), x.clone()).unwrap();
    let p1r = EvidencePair::new(x.clone(), honest.clone()).unwrap();
    assert_eq!(p1.ids(), p1r.ids(), "argument order does not matter");
    let p2 = EvidencePair::new(honest.clone(), y.clone()).unwrap();
    let p3 = EvidencePair::new(x.clone(), y.clone()).unwrap();
    let lowest = p1.clone().lowest(p2.clone()).lowest(p3.clone());
    let lowest_rev = p3.lowest(p2).lowest(p1);
    assert_eq!(lowest.ids(), lowest_rev.ids(), "lowest is independent of comparison order");
    assert!(EvidencePair::new(honest.clone(), honest).is_err(), "identical records are not evidence");
}

#[test]
fn evidence_pair_rejects_other_authors() {
    require_sut!(Core);
    let mut a = Log::new("cs-fa");
    let mut b = Log::new("cs-fb");
    let ra = rec(&a.profile("A", "buyer").wire());
    let rb = rec(&b.profile("B", "buyer").wire());
    assert!(EvidencePair::new(ra, rb).is_err(), "records by two authors prove nothing");
}
