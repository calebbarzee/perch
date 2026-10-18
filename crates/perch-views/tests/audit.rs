//! Regression tests for the independent audit (scratchpad/coord/audit.md)
//! and the usability review fields.

mod common;

use common::*;
use perch_core::ProductKey;
use perch_demo::Author;
use perch_store::{Insert, Store};
use perch_views::{AuthorStatus, Status};
use serde_json::json;

/// Audit #1: a redact notice on a body-less chain member must not panic any view.
#[test]
fn redact_on_bodyless_chain_member_does_not_panic() {
    for target_root in [true, false] {
        let mut w = World::new();
        let mut maker = w.key("maker");
        let key = w.product(&mut maker, "Victim Widget", G1);
        let mut atk = w.key("attacker");
        let ts = w.tick();
        let l1 = atk.publish(ts, "listing", listing_body(&key, "https://x.example/a", 1), None);
        let ts = w.tick();
        let l2 = atk.publish(ts, "listing", listing_body(&key, "https://x.example/a", 2), Some(l1.id()));
        w.insert(&l1.without_body());
        w.insert(&l2.without_body());
        let target = if target_root { l1.id() } else { l2.id() };
        w.put(&mut atk, "redact", json!({ "target": target.to_hex(), "reason": "other", "product": key.as_str() }));
        let v = w.views();
        let pv = v.product(key.as_str()).expect("product view");
        assert_eq!(pv.name.value, "Victim Widget");
        if target_root {
            // The notice names the product of a body-less root: shown as unavailable.
            assert_eq!(pv.listings.len(), 1);
            assert_eq!(pv.listings[0].status, Status::Unavailable);
            assert_eq!(pv.listings[0].redaction.as_ref().unwrap().by, Some(atk.id()));
        } else {
            assert!(pv.listings.is_empty(), "a notice on a non-root member places nothing");
        }
        assert!(v.listing(&l1.id()).is_some() == target_root);
        let _ = v.flags(&l1.id());
        let _ = v.record_status(&l2.id());
        let _ = perch_views::related_records(&w.store, key.as_str(), &w.policy);
        let _ = v.search("widget", &Default::default());
    }
}

/// Audit #2: fixed fields and product key come from the root; a withheld
/// root body cannot later move a chain or strip verified purchase.
#[test]
fn withheld_root_body_cannot_move_chain() {
    let mut w = World::new();
    let mut maker = w.key("maker");
    let ka = w.product(&mut maker, "Widget A", G1);
    let kb = w.product(&mut maker, "Widget B", G2);
    let mut seller = w.key("seller");
    w.verify(&seller, "seller");
    let ts = w.tick();
    let l1 = seller.publish(ts, "listing", listing_body(&ka, "https://shop.example/a", 1000), None);
    w.insert(&l1.without_body());
    let l2 = w.put_r(&mut seller, "listing", listing_body(&kb, "https://shop.example/b", 900), Some(l1.id()));
    let mut buyer = w.key("buyer");
    w.verify(&buyer, "buyer");
    let rc = receipt(&seller, l1.id(), &buyer);
    w.review(&mut buyer, &kb, 1, Some(&rc));

    let v = w.views();
    let pb = v.product(kb.as_str()).unwrap();
    assert!(pb.listings.is_empty(), "the chain's root has no body, so the chain is on no product");
    assert_eq!(pb.reviews[0].purchase, "receipt_invalid");
    assert_eq!(v.record_status(&l2.id()).unwrap().reason.as_deref(), Some("root_unavailable"));
    drop(v);

    // The seller releases the root body later.
    assert_eq!(w.store.insert(l1.clone()), Insert::BodyFilled);
    let v = w.views();
    let pb = v.product(kb.as_str()).unwrap();
    let pa = v.product(ka.as_str()).unwrap();
    assert!(pb.listings.is_empty());
    assert_eq!(pa.listings.len(), 1);
    assert_eq!(pa.listings[0].latest, l1.id(), "the replacement that changed product is invalid");
    assert_eq!(v.record_status(&l2.id()).unwrap().reason.as_deref(), Some("fixed:product"));
    assert_eq!(pb.reviews[0].purchase, "wrong_product");
}

/// Audit #3: one fresh key cannot un-hide a product hidden by verified flags.
#[test]
fn fresh_key_cannot_unhide_hidden_product() {
    let mut w = World::new();
    let mut maker = w.key("maker");
    let p = w.put(&mut maker, "product", json!({ "name": "Counterfeit Widget", "brand": "Acme", "gtin": G1 }));
    for mut f in w.flaggers(5, "fl") {
        w.flag(&mut f, p.id());
    }
    let key = ProductKey::from_gtin(G1).unwrap();
    assert_eq!(w.views().product(key.as_str()).unwrap().moderation.status, Status::Hidden);
    let mut sybil = w.key("sybil");
    w.put(&mut sybil, "product", json!({ "name": "Genuine Widget", "brand": "Acme", "gtin": G1 }));
    let v = w.views();
    let pv = v.product(key.as_str()).unwrap();
    assert_eq!(pv.moderation.status, Status::Hidden);
    assert_eq!(pv.name.value, "Genuine Widget", "facts come from visible chains only");
    assert_eq!(pv.excluded_sources.hidden, 1);
    let hits = v.search("genuine", &Default::default());
    assert_eq!(hits[0].moderation_status, Status::Hidden);
    drop(v);
    // A verified author's visible product record does make it visible.
    let mut verified = w.key("verified");
    w.verify(&verified, "verified");
    w.put(&mut verified, "product", json!({ "name": "Widget", "brand": "Acme", "gtin": G1 }));
    assert_eq!(w.views().product(key.as_str()).unwrap().moderation.status, Status::Visible);
}

/// Audit #7: search matches every candidate value, so keys cannot rename a
/// product out of search.
#[test]
fn search_matches_every_candidate_value() {
    let mut w = World::new();
    let mut maker = w.key("maker");
    w.put(&mut maker, "product", json!({ "name": "Alder Kettle", "brand": "Acme", "gtin": G1 }));
    for i in 0..2 {
        let mut s = w.key(&format!("syb{i}"));
        w.put(&mut s, "product", json!({ "name": "Cheap Deal Kettle", "brand": "Acme", "gtin": G1 }));
    }
    let v = w.views();
    let key = ProductKey::from_gtin(G1).unwrap();
    assert_eq!(v.product(key.as_str()).unwrap().name.value, "Cheap Deal Kettle");
    assert_eq!(v.search("alder", &Default::default()).len(), 1);
    assert_eq!(v.search("deal", &Default::default()).len(), 1);
    assert_eq!(v.search("alder", &Default::default())[0].name, "Cheap Deal Kettle");
}

/// Audit #11: integers above 2^53 - 1 are rejected; the hash never panics.
#[test]
fn policy_large_integers_rejected_and_hash_total() {
    let w = World::new();
    let mut p = w.policy.to_value();
    p["flag_threshold"] = json!(9_007_199_254_740_992u64);
    assert!(perch_views::Policy::from_value(p).is_err());
    let mut p2 = w.policy.to_value();
    p2["trusted_attestors"][0]["not_after_seq"] = json!(10_000_000_000_000_000u64);
    assert!(perch_views::Policy::from_value(p2).is_err());
    let mut raw = w.policy.clone();
    raw.flag_threshold = u64::MAX;
    assert_eq!(perch_views::policy_hash(&raw).len(), 64);
}

/// Audit #8: admission tiers do not depend on product facts; an unverified
/// maker is tier 3 unless a tier-1 record references it.
#[test]
fn admission_tier_unchanged_for_unverified_makers() {
    let mut w = World::new();
    let mut maker = w.key("maker");
    let key = w.product(&mut maker, "Widget", G1);
    let mut v1 = w.key("verified");
    w.verify(&v1, "v1");
    assert_eq!(perch_views::admission_tier(&w.store, &w.policy, &maker.id()), perch_views::Tier::Three);
    let mut s = w.key("seller");
    let l = w.listing(&mut s, &key, "https://s.example/w", 100);
    w.flag(&mut v1, l.id());
    assert_eq!(perch_views::admission_tier(&w.store, &w.policy, &s.id()), perch_views::Tier::Two);
    assert_eq!(perch_views::admission_tier(&w.store, &w.policy, &v1.id()), perch_views::Tier::One);
}

/// UX B1: listings that feed prices come first; the reason is shown.
#[test]
fn feeding_listings_first_with_reasons() {
    let mut w = World::new();
    let mut s = w.key("seller");
    w.verify(&s, "s");
    w.domain(&s, "shop.example");
    let mut phish = w.key("phish");
    let key = w.product(&mut s, "P", G1);
    w.listing(&mut phish, &key, "https://sh0p.example/p", 100);
    w.put(&mut s, "listing", json!({ "product": key.as_str(), "url": "https://shop.example/q",
        "price": { "amount": 50, "currency": "EUR" }, "availability": "out_of_stock", "condition": "new" }));
    w.listing(&mut s, &key, "https://shop.example/p", 1000);
    let pv = w.views().product(key.as_str()).unwrap();
    let got: Vec<(bool, Option<&str>, u64)> =
        pv.listings.iter().map(|l| (l.feeds_prices, l.price_excluded_reason, l.price.as_ref().unwrap().amount)).collect();
    assert_eq!(got, vec![(true, None, 1000), (false, Some("availability"), 50), (false, Some("domain_unverified"), 100)]);
}

/// UX M7: a person restricted because a key forked is explained as such.
#[test]
fn restriction_reason_forked_key() {
    let mut w = World::new();
    let mut m = w.key("m");
    let m2 = w.key("m2");
    w.verify(&m, "m");
    w.verify(&m2, "m");
    w.put(&mut m, "profile", json!({ "name": "M", "role": "seller" }));
    let twin = m.publish_conflicting(w.tick(), "profile", json!({ "name": "M2", "role": "seller" }), None);
    w.insert(&twin);
    let a = w.views().author(&m2.id()).unwrap();
    assert!(a.restricted);
    assert_eq!(a.restriction.unwrap().reason, "forked_key");
    assert_eq!(w.views().author(&m.id()).unwrap().status, AuthorStatus::Forked);
    let _ = Author::from_label("unused");
}
