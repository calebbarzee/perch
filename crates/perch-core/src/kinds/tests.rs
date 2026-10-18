use super::*;
use crate::{Keypair, Unsigned, sign_receipt, sign_record};
use serde_json::json;

fn m(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn err(kind: &str, v: Value) -> SchemaError {
    parse_body(kind, &m(v)).unwrap_err()
}

fn ok(kind: &str, v: Value) -> Body {
    parse_body(kind, &m(v)).unwrap_or_else(|e| panic!("{kind}: {e:?}"))
}

fn field(kind: &str, v: Value) -> (String, Problem) {
    let e = err(kind, v);
    (e.field, e.problem)
}

/// Returns `base` with `key` set to `value` (or removed when `value` is
/// `None`).
fn with(base: &Value, key: &str, value: Option<Value>) -> Value {
    let mut b = base.clone();
    match value {
        Some(v) => {
            b[key] = v;
        }
        None => {
            b.as_object_mut().unwrap().remove(key);
        }
    }
    b
}

const H: &str = "abababababababababababababababababababababababababababababababab";
const GTIN: &str = "gtin:04006381333931";

fn price() -> Value {
    json!({"amount": 1999, "currency": "EUR"})
}

// ---------------------------------------------------------------- shared rules

#[test]
fn unknown_kind_and_unknown_fields() {
    let b = ok("future.kind", json!({"anything": [1, 2]}));
    assert_eq!(b, Body::Unknown(m(json!({"anything": [1, 2]}))));
    assert_eq!(b.kind(), None);
    let b = ok("profile", json!({"name": "A", "role": "seller", "extra": {"deep": 1}}));
    assert_eq!(b.kind(), Some("profile"));
    // Unknown fields inside nested objects are ignored too.
    ok("listing", json!({"product": GTIN, "url": "https://a.example/", "price": {"amount": 1, "currency": "EUR", "x": 1},
        "availability": "in_stock", "condition": "new", "location": {"country": "DE", "x": 2}}));
}

#[test]
fn null_is_not_absent() {
    assert_eq!(field("profile", json!({"name": "A", "role": "seller", "about": null})).0, "about");
    assert_eq!(
        field("profile", json!({"name": null, "role": "seller"})),
        ("name".into(), Problem::WrongType { expected: "non-null value" })
    );
}

#[test]
fn lengths_count_scalar_values() {
    // 100 two-byte characters are allowed; 101 are not.
    ok("profile", json!({"name": "é".repeat(100), "role": "buyer"}));
    assert_eq!(field("profile", json!({"name": "é".repeat(101), "role": "buyer"})), ("name".into(), Problem::TooLong { max: 100 }));
    // A 4-byte emoji counts once.
    ok("profile", json!({"name": "😀".repeat(100), "role": "buyer"}));
    // "e" + combining acute = 2 scalar values.
    assert_eq!(
        field("profile", json!({"name": "e\u{301}".repeat(51), "role": "buyer"})),
        ("name".into(), Problem::TooLong { max: 100 })
    );
    assert_eq!(field("profile", json!({"name": "", "role": "buyer"})), ("name".into(), Problem::TooShort { min: 1 }));
}

#[test]
fn integer_rules() {
    let base = json!({"product": GTIN, "rating": 5, "text": "t"});
    for (v, p) in [
        (json!(0), Problem::OutOfRange { min: 1, max: 5 }),
        (json!(6), Problem::OutOfRange { min: 1, max: 5 }),
        (json!(-1), Problem::OutOfRange { min: 1, max: 5 }),
        (json!(4.5), Problem::WrongType { expected: "integer" }),
        (json!("5"), Problem::WrongType { expected: "integer" }),
        (json!(true), Problem::WrongType { expected: "integer" }),
    ] {
        assert_eq!(field("review", with(&base, "rating", Some(v))), ("rating".into(), p));
    }
    let a = json!({"subject": H, "valid_through_seq": 0});
    ok("attest.revoke", a.clone());
    assert_eq!(field("attest.revoke", with(&a, "valid_through_seq", Some(json!(1u64 << 53)))).0, "valid_through_seq");
}

// ---------------------------------------------------------------- per kind

#[test]
fn profile() {
    let base = json!({"name": "Ada", "role": "node", "url": "https://ada.example/", "about": "x"});
    let Body::Profile(p) = ok("profile", base.clone()) else { panic!() };
    assert_eq!(p.role, Role::Node);
    for r in ROLES {
        ok("profile", with(&base, "role", Some(json!(r))));
    }
    assert!(matches!(field("profile", with(&base, "role", Some(json!("admin")))).1, Problem::NotAllowed { .. }));
    assert!(matches!(field("profile", with(&base, "role", Some(json!("Seller")))).1, Problem::NotAllowed { .. }));
    assert_eq!(field("profile", with(&base, "role", None)), ("role".into(), Problem::Missing));
    assert!(matches!(field("profile", with(&base, "url", Some(json!("http://ada.example/")))).1, Problem::BadUrl { detail: UrlError::NotHttps }));
    assert_eq!(field("profile", with(&base, "about", Some(json!("a".repeat(2001))))).1, Problem::TooLong { max: 2000 });
    ok("profile", with(&base, "about", Some(json!(""))));
}

#[test]
fn attest_identity() {
    let base = json!({"subject": H, "person": H, "level": "person", "country": "DE", "expires": 1_800_000_000_000u64});
    let Body::AttestIdentity(a) = ok("attest.identity", base.clone()) else { panic!() };
    assert_eq!(a.level, Level::Person);
    assert_eq!(field("attest.identity", with(&base, "person", None)), ("person".into(), Problem::Missing));
    assert_eq!(field("attest.identity", with(&base, "person", Some(json!(H.to_uppercase())))), ("person".into(), Problem::BadHex));
    assert_eq!(field("attest.identity", with(&base, "subject", Some(json!("ab")))), ("subject".into(), Problem::BadHex));
    assert_eq!(field("attest.identity", with(&base, "country", Some(json!("de")))), ("country".into(), Problem::BadCountry));
    assert_eq!(field("attest.identity", with(&base, "country", Some(json!("DEU")))), ("country".into(), Problem::BadCountry));
    assert!(matches!(field("attest.identity", with(&base, "level", Some(json!("bot")))).1, Problem::NotAllowed { .. }));
    assert_eq!(field("attest.identity", with(&base, "expires", None)), ("expires".into(), Problem::Missing));
    // legal_name only for business.
    assert_eq!(
        field("attest.identity", with(&base, "legal_name", Some(json!("Ada GmbH")))),
        ("legal_name".into(), Problem::OnlyForBusiness)
    );
    let biz = with(&with(&base, "level", Some(json!("business"))), "legal_name", Some(json!("Ada GmbH")));
    let Body::AttestIdentity(a) = ok("attest.identity", biz.clone()) else { panic!() };
    assert_eq!(a.legal_name.as_deref(), Some("Ada GmbH"));
    assert_eq!(field("attest.identity", with(&biz, "legal_name", Some(json!("")))).1, Problem::TooShort { min: 1 });
    assert_eq!(field("attest.identity", with(&biz, "legal_name", Some(json!("a".repeat(201))))).1, Problem::TooLong { max: 200 });
}

#[test]
fn attest_domain() {
    let base = json!({"subject": H, "domain": "shop.example.com", "expires": 1});
    ok("attest.domain", base.clone());
    ok("attest.domain", with(&base, "domain", Some(json!("xn--bcher-kva.example"))));
    for bad in ["Shop.example.com", "bücher.example", "example", "1.2.3.4", "a..b", "-a.com", "example.com."] {
        assert_eq!(field("attest.domain", with(&base, "domain", Some(json!(bad)))).0, "domain", "{bad}");
    }
}

#[test]
fn attest_revoke() {
    ok("attest.revoke", json!({"subject": H, "valid_through_seq": 12}));
    assert_eq!(field("attest.revoke", json!({"subject": H})), ("valid_through_seq".into(), Problem::Missing));
}

#[test]
fn product() {
    let base = json!({
        "name": "Kettle", "brand": "Acme", "gtin": "4006381333931", "mpn": "K-1",
        "category": "home/kitchen/kettles", "attributes": {"volume": "1.7 l", "colour": "red"},
        "images": [{"url": "https://img.example/k.jpg", "sha256": H}, {"url": "https://img.example/2.jpg"}],
        "description": "A kettle."
    });
    let Body::Product(p) = ok("product", base.clone()) else { panic!() };
    assert_eq!(p.key(RecordId([0; 32])).as_str(), GTIN);
    assert_eq!(p.attributes.as_ref().unwrap().len(), 2);
    // GTIN variants.
    for g in ["96385074", "036000291452", "4006381333931", "00012345678905"] {
        ok("product", with(&base, "gtin", Some(json!(g))));
    }
    assert!(matches!(field("product", with(&base, "gtin", Some(json!("4006381333932")))).1, Problem::BadGtin { detail: ProductKeyError::GtinCheckDigit }));
    assert!(matches!(field("product", with(&base, "gtin", Some(json!("12345")))).1, Problem::BadGtin { detail: ProductKeyError::GtinFormat }));
    assert!(matches!(field("product", with(&base, "gtin", Some(json!(4006381333931u64)))).1, Problem::WrongType { .. }));
    // Name and brand.
    assert_eq!(field("product", with(&base, "name", Some(json!("a".repeat(301))))).1, Problem::TooLong { max: 300 });
    assert_eq!(field("product", with(&base, "brand", Some(json!("")))).1, Problem::TooShort { min: 1 });
    assert_eq!(field("product", with(&base, "brand", None)).1, Problem::Missing);
    assert_eq!(field("product", with(&base, "mpn", Some(json!("")))).1, Problem::TooShort { min: 1 });
    // mpn key must be derivable when there is no gtin.
    let no_gtin = with(&base, "gtin", None);
    let Body::Product(p) = ok("product", no_gtin.clone()) else { panic!() };
    assert_eq!(p.key(RecordId([0; 32])).as_str(), "mpn:acme:k-1");
    assert_eq!(field("product", with(&no_gtin, "mpn", Some(json!("   ")))).0, "mpn");
    // rec key when neither gtin nor mpn.
    let bare = with(&no_gtin, "mpn", None);
    let root = RecordId([7; 32]);
    assert_eq!(derive_product_key(&m(bare.clone()), root).unwrap(), ProductKey::from_root(root));
    // Category.
    for good in ["kitchen", "home/kitchen", "küche/wasserkocher", "a b/c", "ärger/Ä"] {
        ok("product", with(&base, "category", Some(json!(good))));
    }
    for bad in ["Home", "home/", "/home", "home//x", "home/ x", "", "küche/Wasserkocher"] {
        assert_eq!(field("product", with(&base, "category", Some(json!(bad)))).1, Problem::BadCategory, "{bad}");
    }
    assert_eq!(field("product", with(&base, "category", Some(json!("a".repeat(201))))).1, Problem::TooLong { max: 200 });
    // Attributes.
    let many: Map<String, Value> = (0..65).map(|i| (format!("k{i}"), json!("v"))).collect();
    assert_eq!(field("product", with(&base, "attributes", Some(Value::Object(many)))).1, Problem::TooMany { max: 64 });
    assert_eq!(field("product", with(&base, "attributes", Some(json!({"": "v"})))).1, Problem::TooShort { min: 1 });
    assert_eq!(field("product", with(&base, "attributes", Some(json!({"k".repeat(65): "v"})))).1, Problem::TooLong { max: 64 });
    assert_eq!(field("product", with(&base, "attributes", Some(json!({"k": "v".repeat(501)})))), ("attributes.k".into(), Problem::TooLong { max: 500 }));
    assert_eq!(field("product", with(&base, "attributes", Some(json!({"k": 1})))), ("attributes.k".into(), Problem::WrongType { expected: "string" }));
    assert_eq!(field("product", with(&base, "attributes", Some(json!([])))).1, Problem::WrongType { expected: "object" });
    // Images.
    let imgs: Vec<Value> = (0..17).map(|_| json!({"url": "https://i.example/"})).collect();
    assert_eq!(field("product", with(&base, "images", Some(Value::Array(imgs)))).1, Problem::TooMany { max: 16 });
    assert_eq!(field("product", with(&base, "images", Some(json!([{"url": "http://i.example/"}])))).0, "images[0].url");
    assert_eq!(field("product", with(&base, "images", Some(json!([{"url": "https://i.example/", "sha256": "x"}])))), ("images[0].sha256".into(), Problem::BadHex));
    assert_eq!(field("product", with(&base, "images", Some(json!(["https://i.example/"])))), ("images[0]".into(), Problem::WrongType { expected: "object" }));
    assert_eq!(field("product", with(&base, "description", Some(json!("a".repeat(5001))))).1, Problem::TooLong { max: 5000 });
}

#[test]
fn listing() {
    let base = json!({
        "product": GTIN, "url": "https://shop.example/p/1", "price": price(), "availability": "in_stock",
        "condition": "refurbished", "title": "T", "ships_to": ["DE", "AT"], "location": {"country": "DE", "region": "Bayern"}
    });
    let Body::Listing(l) = ok("listing", base.clone()) else { panic!() };
    assert_eq!(l.price, Price { amount: 1999, currency: "EUR".into() });
    assert_eq!(l.condition, Condition::Refurbished);
    for a in AVAILABILITIES {
        ok("listing", with(&base, "availability", Some(json!(a))));
    }
    for c in CONDITIONS {
        ok("listing", with(&base, "condition", Some(json!(c))));
    }
    assert!(matches!(field("listing", with(&base, "product", Some(json!("gtin:4006381333931")))).1, Problem::BadProductKey { .. }));
    assert!(matches!(field("listing", with(&base, "product", Some(json!("mpn:Acme:x")))).1, Problem::BadProductKey { .. }));
    assert!(matches!(field("listing", with(&base, "url", Some(json!("https://127.0.0.1/")))).1, Problem::BadUrl { detail: UrlError::NotDomainHost }));
    assert!(matches!(field("listing", with(&base, "url", Some(json!("https://u:p@shop.example/")))).1, Problem::BadUrl { detail: UrlError::Credentials }));
    // Price.
    assert_eq!(field("listing", with(&base, "price", Some(json!({"amount": 1, "currency": "eur"})))), ("price.currency".into(), Problem::BadCurrency));
    assert_eq!(field("listing", with(&base, "price", Some(json!({"amount": 1, "currency": "XAU"})))).1, Problem::BadCurrency);
    assert_eq!(field("listing", with(&base, "price", Some(json!({"amount": 1, "currency": "ABC"})))).1, Problem::BadCurrency);
    assert_eq!(field("listing", with(&base, "price", Some(json!({"amount": -1, "currency": "EUR"})))).0, "price.amount");
    assert_eq!(field("listing", with(&base, "price", Some(json!({"currency": "EUR"})))), ("price.amount".into(), Problem::Missing));
    assert_eq!(field("listing", with(&base, "price", Some(json!(5)))), ("price".into(), Problem::WrongType { expected: "object" }));
    ok("listing", with(&base, "price", Some(json!({"amount": 0, "currency": "JPY"}))));
    // ships_to and location.
    let all: Vec<Value> = (0..250).map(|_| json!("DE")).collect();
    ok("listing", with(&base, "ships_to", Some(Value::Array(all.clone()))));
    let mut too_many = all;
    too_many.push(json!("DE"));
    assert_eq!(field("listing", with(&base, "ships_to", Some(Value::Array(too_many)))).1, Problem::TooMany { max: 250 });
    assert_eq!(field("listing", with(&base, "ships_to", Some(json!(["DE", "at"])))), ("ships_to[1]".into(), Problem::BadCountry));
    assert_eq!(field("listing", with(&base, "ships_to", Some(json!("DE")))).1, Problem::WrongType { expected: "array" });
    assert_eq!(field("listing", with(&base, "location", Some(json!({"region": "x"})))), ("location.country".into(), Problem::Missing));
    assert_eq!(field("listing", with(&base, "location", Some(json!({"country": "DE", "region": "r".repeat(101)})))).0, "location.region");
    assert_eq!(field("listing", with(&base, "title", Some(json!("t".repeat(301))))).1, Problem::TooLong { max: 300 });
}

#[test]
fn observation() {
    let base = json!({"product": GTIN, "url": "https://shop.example/", "price": price(), "observed_at": 5, "listing": H, "availability": "out_of_stock"});
    let Body::Observation(o) = ok("observation", base.clone()) else { panic!() };
    assert_eq!(o.availability, Some(Availability::OutOfStock));
    assert_eq!(field("observation", with(&base, "listing", Some(json!("zz")))), ("listing".into(), Problem::BadHex));
    assert!(matches!(field("observation", with(&base, "availability", Some(json!("gone")))).1, Problem::NotAllowed { .. }));
    assert_eq!(field("observation", with(&base, "observed_at", None)).1, Problem::Missing);
}

#[test]
fn review_and_receipt() {
    let seller = Keypair::from_seed([1; 32]);
    let buyer = Keypair::from_seed([2; 32]);
    let receipt = sign_receipt(&seller, RecordId([3; 32]), buyer.author(), 100, [4; 32]).unwrap();
    let base = json!({"product": GTIN, "rating": 4, "text": "Good", "title": "T", "listing": H, "receipt": receipt.to_value()});
    let Body::Review(r) = ok("review", base.clone()) else { panic!() };
    assert_eq!(r.receipt.as_ref().unwrap(), &receipt);
    assert!(r.receipt.unwrap().verify(&seller.author()).is_ok());
    assert_eq!(field("review", with(&base, "text", Some(json!("")))).1, Problem::TooShort { min: 1 });
    assert_eq!(field("review", with(&base, "text", Some(json!("a".repeat(10_001))))).1, Problem::TooLong { max: 10_000 });
    ok("review", with(&base, "text", Some(json!("a".repeat(10_000)))));
    assert_eq!(field("review", with(&base, "title", Some(json!("a".repeat(201))))).1, Problem::TooLong { max: 200 });
    let mut bad = receipt.to_value();
    bad["x"] = json!(1);
    assert!(matches!(field("review", with(&base, "receipt", Some(bad))).1, Problem::BadReceipt { .. }));
    assert!(matches!(field("review", with(&base, "receipt", Some(json!("r")))).1, Problem::BadReceipt { detail: ReceiptError::NotObject }));
}

#[test]
fn flag_appeal_retract_retire_redact() {
    let Body::Flag(f) = ok("flag", json!({"target": H, "reason": "personal_data", "note": "n"})) else { panic!() };
    assert_eq!(f.reason, FlagReason::PersonalData);
    for r in FLAG_REASONS {
        ok("flag", json!({"target": H, "reason": r}));
    }
    assert!(matches!(field("flag", json!({"target": H, "reason": "rude"})).1, Problem::NotAllowed { .. }));
    assert_eq!(field("flag", json!({"reason": "spam"})), ("target".into(), Problem::Missing));
    assert_eq!(field("flag", json!({"target": H, "reason": "spam", "note": "a".repeat(2001)})).1, Problem::TooLong { max: 2000 });

    ok("appeal", json!({"target": H, "note": "please"}));
    assert_eq!(field("appeal", json!({"target": H, "note": ""})).1, Problem::TooShort { min: 1 });
    assert_eq!(field("appeal", json!({"target": H})).1, Problem::Missing);

    ok("retract", json!({"target": H}));
    assert_eq!(field("retract", json!({"target": H.to_uppercase()})).1, Problem::BadHex);

    let Body::Retire(r) = ok("retire", json!({})) else { panic!() };
    assert_eq!(r.successor, None);
    ok("retire", json!({"successor": H}));
    assert_eq!(field("retire", json!({"successor": "x"})).1, Problem::BadHex);

    let Body::Redact(r) = ok("redact", json!({"target": H, "reason": "court_order"})) else { panic!() };
    assert_eq!(r.reason, RedactReason::CourtOrder);
    for x in REDACT_REASONS {
        ok("redact", json!({"target": H, "reason": x}));
    }
    assert!(matches!(field("redact", json!({"target": H, "reason": "spam"})).1, Problem::NotAllowed { .. }));
}

// ---------------------------------------------------------------- metadata and record-level

#[test]
fn kind_metadata() {
    assert!(KINDS.windows(2).all(|w| w[0].name < w[1].name));
    assert_eq!(KINDS.len(), 13);
    let control: Vec<_> = KINDS.iter().filter(|k| k.control).map(|k| k.name).collect();
    assert_eq!(control, ["attest.domain", "attest.identity", "attest.revoke", "retire", "retract"]);
    let not_replaceable: Vec<_> = KINDS.iter().filter(|k| !k.replaceable).map(|k| k.name).collect();
    assert_eq!(not_replaceable, ["attest.revoke", "observation", "redact", "retire", "retract"]);
    let not_retractable: Vec<_> = KINDS.iter().filter(|k| !k.retractable).map(|k| k.name).collect();
    assert_eq!(not_retractable, ["attest.revoke", "redact", "retire", "retract"]);
    assert!(is_control_kind("retract"));
    assert!(!is_control_kind("redact"));
    assert!(!is_control_kind("unknown"));
    assert!(!is_replaceable("unknown"));
    assert!(is_retractable("unknown"));
    assert!(is_replaceable("listing"));
}

#[test]
fn fixed_fields_rules() {
    let p_mpn = m(json!({"name": "a", "brand": "Acme", "mpn": "X"}));
    let p_gtin = m(json!({"name": "a", "brand": "Acme", "gtin": "4006381333931"}));
    assert_eq!(fixed_fields("product", &p_mpn), ["gtin", "mpn", "brand"]);
    assert_eq!(fixed_fields("product", &p_gtin), ["gtin", "mpn"]);
    assert_eq!(fixed_fields("listing", &Map::new()), ["product"]);
    assert_eq!(fixed_fields("attest.identity", &Map::new()), ["subject", "person"]);
    assert_eq!(fixed_fields("attest.domain", &Map::new()), ["subject", "domain"]);
    assert_eq!(fixed_fields("flag", &Map::new()), ["target"]);
    assert!(fixed_fields("profile", &Map::new()).is_empty());
    assert!(fixed_fields("nope", &Map::new()).is_empty());

    assert!(check_fixed_fields("product", &p_mpn, &m(json!({"name": "b", "brand": "Acme", "mpn": "X"}))).is_ok());
    let e = check_fixed_fields("product", &p_mpn, &m(json!({"name": "a", "brand": "ACME", "mpn": "X"}))).unwrap_err();
    assert_eq!((e.field.as_str(), e.problem), ("brand", Problem::FixedFieldChanged));
    assert!(check_fixed_fields("product", &p_gtin, &m(json!({"name": "a", "brand": "Other", "gtin": "4006381333931"}))).is_ok());
    // Adding a fixed field that was absent is a change.
    let e = check_fixed_fields("product", &p_gtin, &m(json!({"name": "a", "brand": "Acme", "gtin": "4006381333931", "mpn": "Y"}))).unwrap_err();
    assert_eq!(e.field, "mpn");
    let e = check_fixed_fields("listing", &m(json!({"product": GTIN})), &m(json!({"product": "mpn:a:b"}))).unwrap_err();
    assert_eq!(e.field, "product");
}

#[test]
fn record_level_checks() {
    let kp = Keypair::from_seed([5; 32]);
    let sign = |kind: &str, ts: u64, replaces: Option<RecordId>, b: Value| {
        sign_record(&kp, Unsigned { seq: 1, prev: None, ts, kind: kind.into(), replaces, body: m(b) }).unwrap()
    };
    let obs = json!({"product": GTIN, "url": "https://s.example/", "price": price(), "observed_at": 1000});
    assert!(check_record(&sign("observation", 1000, None, obs.clone())).is_ok());
    let e = check_record(&sign("observation", 999, None, obs.clone())).unwrap_err();
    assert_eq!((e.field.as_str(), e.problem), ("observed_at", Problem::AfterRecordTs));
    let e = check_record(&sign("observation", 1000, Some(RecordId([1; 32])), obs)).unwrap_err();
    assert_eq!((e.field.as_str(), e.problem), ("replaces", Problem::NotReplaceable));
    let r = sign("profile", 1, Some(RecordId([1; 32])), json!({"name": "A", "role": "buyer"}));
    assert!(check_record(&r).is_ok());
    let e = check_record(&r.without_body()).unwrap_err();
    assert_eq!((e.field.as_str(), e.problem), ("body", Problem::BodyMissing));
    // Unknown kinds may carry replaces.
    assert!(check_record(&sign("x.y", 1, Some(RecordId([1; 32])), json!({}))).is_ok());
}

#[test]
fn typed_bodies_round_trip_through_maps() {
    let seller = Keypair::from_seed([1; 32]);
    let receipt = sign_receipt(&seller, RecordId([3; 32]), seller.author(), 100, [4; 32]).unwrap();
    let samples = [
        ("profile", json!({"name": "A", "role": "seller", "url": "https://a.example/"})),
        ("attest.identity", json!({"subject": H, "person": H, "level": "business", "legal_name": "L", "country": "FR", "expires": 9})),
        ("attest.domain", json!({"subject": H, "domain": "a.example", "expires": 9})),
        ("attest.revoke", json!({"subject": H, "valid_through_seq": 3})),
        ("product", json!({"name": "n", "brand": "b", "gtin": "96385074", "attributes": {"a": "b"}, "images": [{"url": "https://i.example/", "sha256": H}]})),
        ("listing", json!({"product": GTIN, "url": "https://s.example/", "price": price(), "availability": "preorder", "condition": "used", "ships_to": ["FR"], "location": {"country": "FR"}})),
        ("observation", json!({"product": GTIN, "url": "https://s.example/", "price": price(), "observed_at": 1, "availability": "discontinued"})),
        ("review", json!({"product": GTIN, "rating": 1, "text": "t", "receipt": receipt.to_value()})),
        ("flag", json!({"target": H, "reason": "fraud"})),
        ("appeal", json!({"target": H, "note": "n"})),
        ("retract", json!({"target": H})),
        ("retire", json!({"successor": H})),
        ("redact", json!({"target": H, "reason": "illegal", "note": "n"})),
    ];
    for (kind, v) in samples {
        let b = ok(kind, v.clone());
        assert_eq!(b.kind(), Some(kind));
        assert_eq!(Value::Object(b.to_map()), v, "{kind}");
        assert_eq!(parse_body(kind, &b.to_map()).unwrap(), b);
    }
}

#[test]
fn schema_error_serializes() {
    let e = err("listing", json!({"product": GTIN, "url": "https://s.example/", "price": {"amount": 1, "currency": "eur"}, "availability": "in_stock", "condition": "new"}));
    assert_eq!(serde_json::to_value(&e).unwrap(), json!({"field": "price.currency", "problem": "bad_currency"}));
    let e = err("review", json!({"product": GTIN, "rating": 9, "text": "t"}));
    assert_eq!(serde_json::to_value(&e).unwrap(), json!({"field": "rating", "problem": "out_of_range", "min": 1, "max": 5}));
    let e = err("profile", json!({"name": "A", "role": "x"}));
    assert_eq!(serde_json::to_value(&e).unwrap()["allowed"][0], json!("seller"));
}

#[test]
fn accessors() {
    let l = ok("listing", json!({"product": GTIN, "url": "https://s.example/", "price": price(), "availability": "in_stock", "condition": "new"}));
    assert_eq!(l.product().unwrap().as_str(), GTIN);
    assert_eq!(l.target(), None);
    let f = ok("redact", json!({"target": H, "reason": "other"}));
    assert_eq!(f.target(), Some(RecordId::from_hex(H).unwrap()));
    let a = ok("attest.revoke", json!({"subject": H, "valid_through_seq": 0}));
    assert_eq!(a.subject(), Some(AuthorId::from_hex(H).unwrap()));
}
