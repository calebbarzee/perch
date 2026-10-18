//! Body removal (contract 5.3).

use perch_tests::fixture::Log;
use perch_tests::require_sut;
use perch_tests::sut::{Ingest, Store, author_view, verify};
use perch_tests::world::{EXPIRES, World};
use serde_json::json;

#[test]
fn header_only_record_accepted() {
    require_sut!();
    let mut l = Log::new("red-a");
    let r1 = l.profile("A", "buyer");
    let r2 = l.add("review", json!({ "product": "gtin:04006381333931", "rating": 3, "text": "Personal data here" }));
    let mut s = Store::new();
    assert_eq!(s.ingest(&r1.wire()), Ingest::Inserted);
    assert_eq!(s.ingest(&r2.without_body().wire()), Ingest::Inserted);
    assert_eq!(s.head_seq(&l.author()), 2, "log continuity holds without the body");
}

#[test]
fn body_restored_when_hash_matches() {
    require_sut!();
    let mut l = Log::new("red-b");
    let r = l.profile("A", "buyer");
    let mut s = Store::new();
    s.ingest(&r.without_body().wire());
    s.ingest(&r.wire());
    assert!(s.export().contains("\"body\":"), "body accepted later when its hash matches");
}

#[test]
fn body_with_wrong_hash_rejected() {
    require_sut!();
    let mut l = Log::new("red-c");
    let r = l.profile("A", "buyer");
    let mut s = Store::new();
    s.ingest(&r.without_body().wire());
    let forged = r.wire_edited(|m| {
        m.insert("body".into(), json!({ "name": "Forged", "role": "buyer" }));
    });
    assert!(verify(&forged).is_err());
    s.ingest(&forged);
    assert!(!s.export().contains("Forged"));
}

#[test]
fn control_kind_without_body_has_no_effect() {
    require_sut!();
    // An attestation whose body is missing does not verify its subject (5.3).
    let mut w = World::new();
    let mut a = Log::new("red-ctl");
    a.profile("A", "buyer");
    let att = w.ida.attest_identity(&a.author(), "red-ctl", EXPIRES);
    let mut s = Store::new();
    for r in &w.ida.records {
        if r.id == att.id {
            s.ingest(&r.without_body().wire());
        } else {
            s.ingest(&r.wire());
        }
    }
    for r in &a.records {
        s.ingest(&r.wire());
    }
    assert!(author_view(&s, &w.policy(), &a.author())["verified"].is_null());
}
