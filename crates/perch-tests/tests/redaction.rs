//! Body removal (contract 5.3).

use perch_tests::fixture::Log;
use perch_tests::require_sut;
use perch_tests::sut::{Ingest, Store, author_view, verify};
use perch_tests::world::{EXPIRES, World};
use serde_json::json;

#[test]
fn header_only_record_accepted() {
    require_sut!(Store);
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
    require_sut!(Store);
    let mut l = Log::new("red-b");
    let r = l.profile("A", "buyer");
    let mut s = Store::new();
    s.ingest(&r.without_body().wire());
    s.ingest(&r.wire());
    assert!(s.export().contains("\"body\":"), "body accepted later when its hash matches");
}

#[test]
fn body_with_wrong_hash_rejected() {
    require_sut!(Store);
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
    require_sut!(Views);
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

#[test]
fn control_kind_body_cannot_be_removed() {
    require_sut!(Store);
    let mut w = World::new();
    let a = Log::new("red-ctl2");
    let att = w.ida.attest_identity(&a.author(), "red-ctl2", EXPIRES);
    let mut s = Store::new();
    for r in &w.ida.records {
        s.ingest(&r.wire());
    }
    assert!(s.remove_body(&att.id).is_err(), "attest.identity is a control kind (4.5, 5.3)");
    assert!(s.has_body(&att.id));
}

#[test]
fn removed_body_refused_on_resend() {
    require_sut!(Store);
    let mut l = Log::new("red-list");
    let r = l.add("review", json!({ "product": "gtin:04006381333931", "rating": 3, "text": "Doxxing here" }));
    let mut s = Store::new();
    s.ingest(&r.wire());
    assert_eq!(s.remove_body(&r.id), Ok(true));
    assert!(!s.has_body(&r.id));
    s.ingest(&r.wire());
    assert!(!s.has_body(&r.id), "a removed body must not come back from a peer");
    assert!(!s.export().contains("Doxxing"), "export must not contain the removed body");
}

#[test]
fn removal_list_applies_before_first_receipt() {
    require_sut!(Store);
    let mut l = Log::new("red-pre");
    let r = l.add("review", json!({ "product": "gtin:04006381333931", "rating": 3, "text": "Court order" }));
    let mut s = Store::new();
    s.add_removed(&r.id);
    s.ingest(&r.wire());
    assert!(s.has(&r.id), "header kept for log continuity");
    assert!(!s.has_body(&r.id));
}
