//! Isolated validity (contract 4.2–4.4): tampering and signature checks.

use perch_tests::fixture::{Key, Log, RECEIPT_DOMAIN, canonical, receipt};
use perch_tests::require_sut;
use perch_tests::sut::verify;
use perch_tests::vectors::{malleate_s, weak_key_record};
use serde_json::json;

fn sample() -> perch_tests::fixture::Record {
    let mut l = Log::new("isolated-alice");
    l.profile("Alice", "buyer");
    l.add("review", json!({ "product": "gtin:04006381333931", "rating": 4, "text": "Good." }))
}

#[test]
fn valid_record_verifies_with_expected_id() {
    require_sut!(Core);
    let r = sample();
    assert_eq!(verify(&r.wire()), Ok(r.id.clone()));
}

#[test]
fn tampered_body_rejected() {
    require_sut!(Core);
    let r = sample();
    let w = r.wire_edited(|m| {
        m.get_mut("body").unwrap()["rating"] = json!(5);
    });
    assert!(verify(&w).is_err(), "body change without a matching body_hash must fail");
}

#[test]
fn body_hash_mismatch_rejected() {
    require_sut!(Core);
    let r = sample();
    // Recompute body_hash for the new body but keep the old signature.
    let w = r.wire_edited(|m| {
        m.get_mut("body").unwrap()["rating"] = json!(1);
        let h = perch_tests::fixture::sha256_hex(canonical(&m["body"]).as_bytes());
        m.insert("body_hash".into(), json!(h));
    });
    assert!(verify(&w).is_err(), "header changed (body_hash), so the signature must fail");
}

#[test]
fn tampered_sig_rejected() {
    require_sut!(Core);
    let r = sample();
    let mut sig = hex::decode(&r.sig).unwrap();
    sig[10] ^= 1;
    let w = r.wire().replace(&r.sig, &hex::encode(sig));
    assert!(verify(&w).is_err());
}

#[test]
fn tampered_author_rejected() {
    require_sut!(Core);
    let r = sample();
    let other = Key::from_label("isolated-mallory").author();
    let w = r.wire().replace(r.author(), &other);
    assert!(verify(&w).is_err());
}

#[test]
fn tampered_seq_rejected() {
    require_sut!(Core);
    let r = sample();
    let w = r.wire_edited(|m| {
        m.insert("seq".into(), json!(7));
    });
    assert!(verify(&w).is_err());
}

#[test]
fn tampered_ts_and_kind_rejected() {
    require_sut!(Core);
    let r = sample();
    let w1 = r.wire_edited(|m| {
        m.insert("ts".into(), json!(1));
    });
    let w2 = r.wire_edited(|m| {
        m.insert("kind".into(), json!("flag"));
    });
    assert!(verify(&w1).is_err());
    assert!(verify(&w2).is_err());
}

#[test]
fn signature_from_other_key_rejected() {
    require_sut!(Core);
    // Mallory signs Alice's header with Mallory's key but keeps Alice as author.
    let r = sample();
    let mallory = Key::from_label("isolated-mallory");
    let hc = canonical(&serde_json::Value::Object(r.header.clone()));
    let sig = mallory.sign_hex(perch_tests::fixture::RECORD_DOMAIN, hc.as_bytes());
    let w = r.wire().replace(&r.sig, &sig);
    assert!(verify(&w).is_err());
}

#[test]
fn signature_without_domain_prefix_rejected() {
    require_sut!(Core);
    let r = sample();
    let k = Key::from_label("isolated-alice");
    let hc = canonical(&serde_json::Value::Object(r.header.clone()));
    let sig = k.sign_hex(b"", hc.as_bytes());
    assert!(verify(&r.wire().replace(&r.sig, &sig)).is_err());
}

#[test]
fn receipt_signature_not_valid_as_record() {
    require_sut!(Core);
    // A header signed under the receipt domain must not verify as a record.
    let r = sample();
    let k = Key::from_label("isolated-alice");
    let hc = canonical(&serde_json::Value::Object(r.header.clone()));
    let sig = k.sign_hex(RECEIPT_DOMAIN, hc.as_bytes());
    assert!(verify(&r.wire().replace(&r.sig, &sig)).is_err());
    // And a record signature must not verify as a receipt.
    let rec = receipt(&k, &r.id, &k.author(), 1, "n");
    let mut forged = rec.clone();
    forged["sig"] = json!(r.sig);
    assert!(perch_tests::sut::verify_receipt(&forged, &k.author()).is_err());
    assert!(perch_tests::sut::verify_receipt(&rec, &k.author()).is_ok());
}

#[test]
fn small_order_public_key_rejected() {
    require_sut!(Core);
    // Lax verification accepts this vector for every message (see vectors.rs test).
    assert!(verify(&weak_key_record()).is_err());
}

#[test]
fn non_canonical_s_rejected() {
    require_sut!(Core);
    let r = sample();
    let w = r.wire().replace(&r.sig, &malleate_s(&r.sig));
    assert!(verify(&w).is_err(), "S + L must be rejected (S >= L)");
}

#[test]
fn header_only_record_verifies() {
    require_sut!(Core);
    // Contract 4.1: body optional on the wire (removed by a node).
    let r = sample().without_body();
    assert_eq!(verify(&r.wire()), Ok(r.id.clone()));
}

#[test]
fn universal_forgery_identity_key_rejected() {
    require_sut!(Core);
    // Key = identity point 01 00..00, sig = R identity || S 0. Valid for every
    // message under cofactorless verification without small-order checks, which
    // Web Crypto (Node 22.22) and Python cryptography 50.0.1 accept.
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let identity = perch_tests::vectors::SMALL_ORDER_POINTS[0];
    let sig_hex = format!("{identity}{}", "00".repeat(32));
    let vk = VerifyingKey::from_bytes(&hex::decode(identity).unwrap().try_into().unwrap()).unwrap();
    let sig = Signature::from_bytes(&hex::decode(&sig_hex).unwrap().try_into().unwrap());
    for msg in [&b"perch/record/v1\n{}"[..], b"anything at all"] {
        assert!(vk.verify(msg, &sig).is_ok(), "precondition: lax verification accepts any message");
    }
    // As a record.
    assert!(verify(&perch_tests::vectors::weak_key_record()).is_err());
    // As a receipt from that "seller".
    let rc = json!({
        "listing": "a".repeat(64), "buyer": "b".repeat(64), "purchased_at": 1,
        "nonce": "c".repeat(64), "sig": sig_hex
    });
    assert!(perch_tests::sut::verify_receipt(&rc, identity).is_err());
    // Every small-order key is rejected the same way.
    for p in perch_tests::vectors::SMALL_ORDER_POINTS {
        let w = perch_tests::vectors::weak_key_record().replace(identity, p);
        assert!(verify(&w).is_err(), "small-order key {p}");
    }
}
