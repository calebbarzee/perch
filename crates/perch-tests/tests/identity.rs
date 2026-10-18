//! Identity and verification (contract 7.1, 7.3).

use perch_tests::fixture::{Log, Opts, T0};
use perch_tests::require_sut;
use perch_tests::sut::{Store, author_view, listing_view};
use perch_tests::world::{EXPIRES, NOW, World, product_and_listing};
use serde_json::{Value, json};

fn store_of(w: &World, logs: &[&Log]) -> Store {
    let mut s = Store::new();
    for l in w.lines(logs) {
        s.ingest(&l);
    }
    s
}

fn verified(s: &Store, p: &Value, author: &str) -> bool {
    let v = author_view(s, p, author);
    assert!(!v.is_null(), "author {author} must exist in the store");
    !v["verified"].is_null()
}

#[test]
fn attested_author_is_verified() {
    require_sut!();
    let mut w = World::new();
    let mut a = w.verified("id-alice", "alice");
    a.profile("Alice", "buyer");
    let s = store_of(&w, &[&a]);
    let v = author_view(&s, &w.policy(), &a.author());
    assert_eq!(v["verified"]["attestor"], json!(w.ida.author()));
    assert_eq!(v["verified"]["level"], json!("person"));
}

#[test]
fn expired_attestation_not_verified() {
    require_sut!();
    let mut w = World::new();
    let mut a = Log::new("id-expired");
    a.profile("A", "buyer");
    w.ida.attest_identity(&a.author(), "expired", NOW); // expires == now: not > now
    let s = store_of(&w, &[&a]);
    assert!(!verified(&s, &w.policy(), &a.author()));
}

#[test]
fn retracted_attestation_not_verified() {
    require_sut!();
    let mut w = World::new();
    let mut a = Log::new("id-retracted");
    a.profile("A", "buyer");
    let att = w.ida.attest_identity(&a.author(), "retracted", EXPIRES);
    w.ida.retract(&att.id);
    let s = store_of(&w, &[&a]);
    assert!(!verified(&s, &w.policy(), &a.author()));
}

#[test]
fn self_issued_attestation_not_verified() {
    require_sut!();
    let w = World::new();
    let mut a = Log::new("id-self");
    let me = a.author();
    a.attest_identity(&me, "self", EXPIRES);
    let s = store_of(&w, &[&a]);
    assert!(!verified(&s, &w.policy(), &a.author()));
}

#[test]
fn untrusted_attestor_not_verified() {
    require_sut!();
    let w = World::new();
    let mut rogue = Log::new("id-rogue-attestor");
    let mut a = Log::new("id-untrusted");
    a.profile("A", "buyer");
    rogue.attest_identity(&a.author(), "untrusted", EXPIRES);
    let s = store_of(&w, &[&rogue, &a]);
    assert!(!verified(&s, &w.policy(), &a.author()));
}

#[test]
fn attestor_trusted_for_other_kind_not_verified() {
    require_sut!();
    // The domain attestor is not trusted for attest.identity.
    let mut w = World::new();
    let mut a = Log::new("id-wrongkind");
    a.profile("A", "buyer");
    w.doma.attest_identity(&a.author(), "wrongkind", EXPIRES);
    let s = store_of(&w, &[&a]);
    assert!(!verified(&s, &w.policy(), &a.author()));
}

#[test]
fn not_after_seq_cuts_off_attestor() {
    require_sut!();
    let mut w = World::new();
    let mut before = Log::new("id-before");
    let mut after = Log::new("id-after");
    before.profile("B", "buyer");
    after.profile("A", "buyer");
    let att1 = w.ida.attest_identity(&before.author(), "before", EXPIRES);
    w.ida.attest_identity(&after.author(), "after", EXPIRES);
    let mut p = w.policy();
    p["trusted_attestors"][0]["not_after_seq"] = json!(att1.seq());
    let s = store_of(&w, &[&before, &after]);
    assert!(verified(&s, &p, &before.author()));
    assert!(!verified(&s, &p, &after.author()), "attestations after the cutoff do not count");
}

#[test]
fn forked_attestor_attestations_after_fork_do_not_count() {
    require_sut!();
    let mut w = World::new();
    let mut a = Log::new("id-fa-a");
    let mut b = Log::new("id-fa-b");
    a.profile("A", "buyer");
    b.profile("B", "buyer");
    w.ida.attest_identity(&a.author(), "fa-a", EXPIRES); // seq 2
    let att_b = w.ida.attest_identity(&b.author(), "fa-b", EXPIRES); // seq 3
    let alt = w.ida.fork_at(att_b.seq(), "profile", json!({ "name": "thief", "role": "attestor" }));
    let mut s = store_of(&w, &[&a, &b]);
    s.ingest(&alt.wire());
    assert!(verified(&s, &w.policy(), &a.author()), "below fork seq still counts");
    assert!(!verified(&s, &w.policy(), &b.author()), "at or above fork seq does not count");
}

#[test]
fn revoke_ignores_later_records() {
    require_sut!();
    let mut w = World::new();
    let mut maker = Log::new("id-rev-maker");
    let mut seller = w.seller("id-rev-seller", "shop.example");
    let (_p, l1) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let cutoff = seller.head().unwrap().seq();
    // Thief publishes a cheaper listing after the cutoff.
    let l2 = seller.listing(&perch_tests::world::key_a(), "https://shop.example/scam", 1);
    w.ida.attest_revoke(&seller.author(), cutoff);
    let s = store_of(&w, &[&maker, &seller]);
    let p = w.policy();
    assert_eq!(author_view(&s, &p, &seller.author())["status"], json!("revoked"));
    assert!(!listing_view(&s, &p, &l1.id).is_null(), "records up to the cutoff remain");
    let v2 = listing_view(&s, &p, &l2.id);
    assert!(v2.is_null() || v2["status"] != json!("visible"), "records after the cutoff are ignored: {v2}");
}

#[test]
fn revoke_from_unrelated_attestor_ignored() {
    require_sut!();
    // A second trusted identity attestor that never attested the subject cannot revoke it.
    let mut w = World::new();
    let mut other = Log::new("id-other-attestor");
    let mut a = w.verified("id-rev-target", "target");
    a.profile("A", "buyer");
    other.attest_revoke(&a.author(), 0);
    let p = perch_tests::fixture::policy(NOW, &[&w.ida.author(), &other.author()], &[&w.doma.author()]);
    let s = store_of(&w, &[&other, &a]);
    assert_eq!(author_view(&s, &p, &a.author())["status"], json!("ok"));
    assert!(verified(&s, &p, &a.author()));
}

#[test]
fn revoke_by_cut_off_attestor_ignored() {
    require_sut!();
    let mut w = World::new();
    let mut a = w.verified("id-cut-target", "cut");
    a.profile("A", "buyer");
    let cutoff = w.ida.head().unwrap().seq();
    w.ida.attest_revoke(&a.author(), 0);
    let mut p = w.policy();
    p["trusted_attestors"][0]["not_after_seq"] = json!(cutoff);
    let s = store_of(&w, &[&a]);
    assert_eq!(author_view(&s, &p, &a.author())["status"], json!("ok"), "revoke after not_after_seq must not count");
}

#[test]
fn stolen_key_fork_does_not_restrict_person_after_revoke() {
    require_sut!();
    // Victim has two keys for one person. A thief forks key 1. After the attestor
    // revokes key 1, key 2 must not be restricted (contract 7.3).
    let mut w = World::new();
    let mut k1 = w.verified("id-victim-k1", "victim");
    let mut k2 = w.verified("id-victim-k2", "victim");
    k1.profile("Victim", "seller");
    k2.profile("Victim", "seller");
    let mut maker = Log::new("id-victim-maker");
    let (_p, l) = product_and_listing(&mut maker, &mut k2, "victim.example", 1000);
    w.doma.attest_domain(&k2.author(), "victim.example", EXPIRES);
    let alt = k1.fork_at(1, "profile", json!({ "name": "thief", "role": "seller" }));

    // Before revoke: forked key restricts the person.
    let mut s = store_of(&w, &[&k1, &k2, &maker]);
    s.ingest(&alt.wire());
    let p = w.policy();
    assert_eq!(author_view(&s, &p, &k1.author())["status"], json!("forked"));
    assert_eq!(listing_view(&s, &p, &l.id)["seller_restricted"], json!(true));

    // After revoke of k1 at seq 0.
    w.ida.attest_revoke(&k1.author(), 0);
    let mut s = store_of(&w, &[&k1, &k2, &maker]);
    s.ingest(&alt.wire());
    assert_eq!(author_view(&s, &p, &k1.author())["status"], json!("revoked"), "revoked > forked");
    assert_eq!(listing_view(&s, &p, &l.id)["seller_restricted"], json!(false));
}

#[test]
fn forked_seller_restricted_on_other_keys() {
    require_sut!();
    let mut w = World::new();
    let mut k1 = w.verified("id-forker-k1", "forker");
    let mut k2 = w.verified("id-forker-k2", "forker");
    k1.profile("F", "seller");
    let mut maker = Log::new("id-forker-maker");
    let (_p, l) = product_and_listing(&mut maker, &mut k2, "forker.example", 1000);
    let alt = k1.fork_at(1, "profile", json!({ "name": "F2", "role": "seller" }));
    let mut s = store_of(&w, &[&k1, &k2, &maker]);
    s.ingest(&alt.wire());
    assert_eq!(listing_view(&s, &w.policy(), &l.id)["seller_restricted"], json!(true));
}

#[test]
fn attestation_in_future_ignored() {
    require_sut!();
    let mut w = World::new();
    let mut a = Log::new("id-future");
    a.profile("A", "buyer");
    w.ida.add_with(
        "attest.identity",
        json!({ "subject": a.author(), "person": perch_tests::fixture::person_hex("future"), "level": "person", "expires": EXPIRES }),
        Opts { ts: Some(NOW + 600_001), ..Default::default() },
    );
    let s = store_of(&w, &[&a]);
    assert!(!verified(&s, &w.policy(), &a.author()), "ts beyond now + tolerance: not yet existing");
    let _ = T0;
}
