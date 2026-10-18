//! Scenario tests: one or more per rule in docs/views.md.

mod common;

use common::*;
use perch_core::ProductKey;
use perch_views::{AuthorStatus, Status, canonical_json};
use serde_json::json;

// ---------------------------------------------------------------------------
// Moderation (7.6)
// ---------------------------------------------------------------------------

fn hidden_listing_world(flags: usize) -> (World, perch_core::RecordId) {
    let mut w = World::new();
    let mut s = w.key("seller");
    w.verify(&s, "seller");
    let key = w.product(&mut s, "P", G1);
    let l = w.listing(&mut s, &key, "https://shop.example/p", 1000);
    for mut f in w.flaggers(flags, "f") {
        w.flag(&mut f, l.id());
    }
    (w, l.id())
}

#[test]
fn verified_flags_hide_at_threshold() {
    let (w, l) = hidden_listing_world(3);
    assert_eq!(w.views().listing(&l).unwrap().status, Status::Hidden);
    let (w, l) = hidden_listing_world(2);
    let v = w.views().listing(&l).unwrap();
    assert_eq!((v.status, v.flags.counted), (Status::Visible, 2));
}

#[test]
fn seller_self_flag_not_counted() {
    let mut w = World::new();
    let mut seller = w.key("seller");
    w.verify(&seller, "seller");
    let key = w.product(&mut seller, "P", G1);
    let l = w.listing(&mut seller, &key, "https://shop.example/p", 1000);
    w.flag(&mut seller, l.id());
    let flags = w.views().flags(&l.id());
    assert_eq!(flags.len(), 1);
    assert_eq!(flags[0].excluded_reason, Some("self"));
}

#[test]
fn seller_other_key_same_person_flag_not_counted() {
    let mut w = World::new();
    let mut seller = w.key("seller");
    let mut alt = w.key("alt");
    w.verify(&seller, "seller-person");
    w.verify(&alt, "seller-person");
    let key = w.product(&mut seller, "P", G1);
    let l = w.listing(&mut seller, &key, "https://shop.example/p", 1000);
    w.flag(&mut alt, l.id());
    assert_eq!(w.views().flags(&l.id())[0].excluded_reason, Some("self"));
}

#[test]
fn product_seller_flags_on_review_not_counted() {
    let mut w = World::new();
    let mut seller = w.key("seller");
    let mut alt = w.key("alt");
    w.verify(&seller, "seller-person");
    w.verify(&alt, "seller-person");
    let mut buyer = w.key("buyer");
    w.verify(&buyer, "buyer");
    let key = w.product(&mut seller, "P", G1);
    w.listing(&mut seller, &key, "https://shop.example/p", 1000);
    let r = w.review(&mut buyer, &key, 1, None);
    w.flag(&mut seller, r.id());
    w.flag(&mut alt, r.id());
    let flags = w.views().flags(&r.id());
    assert_eq!(flags.len(), 2);
    assert!(flags.iter().all(|f| f.excluded_reason == Some("product_seller")), "{flags:?}");
}

#[test]
fn unverified_sybil_flags_not_counted() {
    let mut w = World::new();
    let mut s = w.key("seller");
    let key = w.product(&mut s, "P", G1);
    let l = w.listing(&mut s, &key, "https://shop.example/p", 1000);
    for i in 0..10 {
        let mut k = w.key(&format!("sybil{i}"));
        w.flag(&mut k, l.id());
    }
    let v = w.views().listing(&l.id()).unwrap();
    assert_eq!((v.status, v.flags.count, v.flags.counted), (Status::Visible, 10, 0));
    // With flags_require_verified off, each key is its own person.
    w.policy.flags_require_verified = false;
    assert_eq!(w.views().listing(&l.id()).unwrap().status, Status::Hidden);
}

#[test]
fn one_person_many_keys_flags_count_once() {
    let mut w = World::new();
    let mut s = w.key("seller");
    let key = w.product(&mut s, "P", G1);
    let l = w.listing(&mut s, &key, "https://shop.example/p", 1000);
    for i in 0..5 {
        let mut k = w.key(&format!("k{i}"));
        w.verify(&k, "same-human");
        w.flag(&mut k, l.id());
    }
    let v = w.views().listing(&l.id()).unwrap();
    assert_eq!((v.flags.count, v.flags.counted, v.status), (5, 1, Status::Visible));
}

#[test]
fn person_keys_lists_component() {
    let mut w = World::new();
    let a = w.key("a");
    let b = w.key("b");
    let c = w.key("c");
    w.verify(&a, "h");
    w.verify(&b, "h");
    let mut expect = vec![a.id(), b.id()];
    expect.sort();
    assert_eq!(perch_views::person_keys(&w.store, &w.policy, &b.id()), expect);
    assert_eq!(perch_views::person_keys(&w.store, &w.policy, &c.id()), vec![c.id()]);
}

#[test]
fn person_component_links_through_shared_pairs() {
    // k1 has (A, p); k2 has (A, p) and (B, r); k3 has (B, r): one person.
    let mut w = World::new();
    let mut b = Author::from_label("test/attestor-b");
    w.policy = perch_views::default_policy(NOW, &[w.attestor.id(), b.id()]);
    let k1 = w.key("k1");
    let k2 = w.key("k2");
    let k3 = w.key("k3");
    w.verify(&k1, "p");
    w.verify(&k2, "p");
    w.attest(&mut b, &k2, "r", EXP);
    w.attest(&mut b, &k3, "r", EXP);
    let mut s = w.key("seller");
    let key = w.product(&mut s, "P", G1);
    let l = w.listing(&mut s, &key, "https://shop.example/p", 1000);
    for mut k in [k1, k2, k3] {
        w.flag(&mut k, l.id());
    }
    assert_eq!(w.views().listing(&l.id()).unwrap().flags.counted, 1);
}

#[test]
fn flag_retraction_unhides_and_appeal_alone_does_not() {
    let mut w = World::new();
    let mut s = w.key("seller");
    let key = w.product(&mut s, "P", G1);
    let l = w.listing(&mut s, &key, "https://shop.example/p", 1000);
    let mut fl = w.flaggers(3, "f");
    let ids: Vec<_> = fl.iter_mut().map(|f| w.flag(f, l.id()).id()).collect();
    w.put(&mut s, "appeal", json!({ "target": l.id().to_hex(), "note": "Genuine." }));
    let v = w.views().listing(&l.id()).unwrap();
    assert_eq!((v.status, v.appealed), (Status::Hidden, true));
    w.retract(&mut fl[0], ids[0]);
    let v = w.views().listing(&l.id()).unwrap();
    assert_eq!((v.status, v.flags.counted), (Status::Visible, 2));
    let flags = w.views().flags(&l.id());
    assert!(flags.iter().any(|f| f.excluded_reason == Some("retracted")));
}

#[test]
fn price_update_keeps_flags() {
    let mut w = World::new();
    let mut s = w.key("seller");
    let key = w.product(&mut s, "P", G1);
    let l1 = w.listing(&mut s, &key, "https://shop.example/p", 1000);
    for mut f in w.flaggers(3, "f") {
        w.flag(&mut f, l1.id());
    }
    let l2 = w.put_r(&mut s, "listing", listing_body(&key, "https://shop.example/p", 900), Some(l1.id()));
    let v = w.views().listing(&l2.id()).unwrap();
    assert_eq!((v.id, v.latest, v.status), (l1.id(), l2.id(), Status::Hidden));
    assert_eq!(v.price_history.len(), 2);
}

#[test]
fn per_kind_threshold_for_reviews() {
    let mut w = World::new();
    let mut s = w.key("seller");
    let key = w.product(&mut s, "P", G1);
    let mut a = w.key("author");
    let r = w.review(&mut a, &key, 1, None);
    let mut fl = w.flaggers(5, "f");
    for f in fl.iter_mut().take(4) {
        w.flag(f, r.id());
    }
    let pv = w.views().product(key.as_str()).unwrap();
    assert_eq!(pv.reviews[0].status, Status::Visible, "4 < review threshold 5");
    w.flag(&mut fl[4], r.id());
    let pv = w.views().product(key.as_str()).unwrap();
    assert_eq!(pv.reviews[0].status, Status::Hidden);
    assert!(pv.reviews[0].counted, "hidden reviews still count");
    assert_eq!(pv.ratings.count, 1);
}

#[test]
fn flags_on_other_kinds_hide_nothing() {
    let mut w = World::new();
    let mut s = w.key("seller");
    let key = w.product(&mut s, "P", G1);
    let l = w.listing(&mut s, &key, "https://shop.example/p", 1000);
    let mut f0 = w.flaggers(1, "x").remove(0);
    let flag = w.flag(&mut f0, l.id());
    let mut fl = w.flaggers(5, "f");
    for f in fl.iter_mut() {
        w.flag(f, flag.id());
    }
    let v = w.views();
    assert_eq!(v.chain_status(&flag.id()), Some(Status::Visible));
    assert_eq!(v.flags(&flag.id()).iter().filter(|f| f.counted).count(), 5);
    assert_eq!(v.listing(&l.id()).unwrap().flags.counted, 1, "the flag still counts");
}

fn restriction_world(flaggers_per_listing: usize, overlap: bool) -> (World, Author, Author) {
    let mut w = World::new();
    let mut s = w.key("seller");
    let alt = w.key("alt");
    w.verify(&s, "shady");
    w.verify(&alt, "shady");
    let mut fl = w.flaggers(9, "f");
    for (j, g) in [G1, G2, "4006381333955"].iter().enumerate() {
        let key = w.product(&mut s, "P", g);
        let l = w.listing(&mut s, &key, &format!("https://shady.example/{j}"), 1000);
        for i in 0..flaggers_per_listing {
            let idx = if overlap { i } else { j * flaggers_per_listing + i };
            w.flag(&mut fl[idx], l.id());
        }
    }
    (w, s, alt)
}

#[test]
fn restriction_requires_distinct_flaggers() {
    let (w, s, _) = restriction_world(3, true);
    let a = w.views().author(&s.id()).unwrap();
    assert_eq!(a.hidden_listing_count, 3);
    assert!(!a.restricted, "3 hidden listings but only 3 distinct flaggers");
    let (w, s, _) = restriction_world(3, false);
    let a = w.views().author(&s.id()).unwrap();
    assert!(a.restricted);
    assert_eq!(a.restriction.unwrap().distinct_flaggers, 9);
}

#[test]
fn restriction_follows_person_to_new_key() {
    let (mut w, _, mut alt) = restriction_world(3, false);
    let key = ProductKey::from_gtin(G1).unwrap();
    let l = w.listing(&mut alt, &key, "https://outlet.shady.example/x", 900);
    let v = w.views().listing(&l.id()).unwrap();
    assert!(v.seller_restricted);
    assert!(w.views().author(&alt.id()).unwrap().restricted);
}

#[test]
fn restriction_survives_retraction_of_hidden_listings() {
    let (mut w, mut s, _) = restriction_world(3, false);
    let listings = perch_store::Store::by_author_kind(&w.store, &s.id(), "listing");
    for l in listings {
        w.retract(&mut s, l);
    }
    assert!(w.views().author(&s.id()).unwrap().restricted);
}

#[test]
fn hidden_listing_still_anchors_receipts() {
    let mut w = World::new();
    let mut s = w.key("seller");
    w.verify(&s, "seller");
    let mut b = w.key("buyer");
    w.verify(&b, "buyer");
    let key = w.product(&mut s, "P", G1);
    let l = w.listing(&mut s, &key, "https://shop.example/p", 1000);
    for mut f in w.flaggers(3, "f") {
        w.flag(&mut f, l.id());
    }
    let rc = receipt(&s, l.id(), &b);
    w.review(&mut b, &key, 5, Some(&rc));
    let pv = w.views().product(key.as_str()).unwrap();
    assert_eq!(pv.listings[0].status, Status::Hidden);
    assert_eq!(pv.reviews[0].purchase, "verified");
    assert!(pv.price_range.is_empty(), "hidden listings feed no prices");
}

// ---------------------------------------------------------------------------
// Verified purchase (7.5)
// ---------------------------------------------------------------------------

struct Vp {
    w: World,
    seller: Author,
    buyer: Author,
    key: ProductKey,
    other: ProductKey,
    listing: perch_core::RecordId,
    other_listing: perch_core::RecordId,
}

fn vp_world(seller_verified: bool, buyer_verified: bool) -> Vp {
    let mut w = World::new();
    let mut seller = w.key("seller");
    let buyer = w.key("buyer");
    if seller_verified {
        w.verify(&seller, "seller");
    }
    if buyer_verified {
        w.verify(&buyer, "buyer");
    }
    let key = w.product(&mut seller, "P", G1);
    let other = w.product(&mut seller, "Q", G2);
    let listing = w.listing(&mut seller, &key, "https://shop.example/p", 1000).id();
    let other_listing = w.listing(&mut seller, &other, "https://shop.example/q", 1000).id();
    Vp { w, seller, buyer, key, other, listing, other_listing }
}

fn purchase_of(vp: &mut Vp, rc: &perch_core::Receipt) -> &'static str {
    let key = vp.key.clone();
    let mut buyer = vp.buyer.clone();
    vp.w.review(&mut buyer, &key, 4, Some(rc));
    vp.buyer = buyer;
    let pv = vp.w.views().product(key.as_str()).unwrap();
    pv.reviews.iter().find(|r| r.author == vp.buyer.id()).unwrap().purchase
}

#[test]
fn valid_receipt_is_verified_purchase() {
    let mut vp = vp_world(true, true);
    let rc = receipt(&vp.seller, vp.listing, &vp.buyer);
    assert_eq!(purchase_of(&mut vp, &rc), "verified");
}

#[test]
fn no_receipt_is_none() {
    let mut vp = vp_world(true, true);
    let key = vp.key.clone();
    vp.w.review(&mut vp.buyer, &key, 4, None);
    assert_eq!(vp.w.views().product(key.as_str()).unwrap().reviews[0].purchase, "none");
}

#[test]
fn receipt_by_non_seller_not_verified() {
    let mut vp = vp_world(true, true);
    let impostor = Author::from_label("test/impostor");
    let rc = receipt(&impostor, vp.listing, &vp.buyer);
    assert_eq!(purchase_of(&mut vp, &rc), "receipt_invalid");
}

#[test]
fn tampered_receipt_not_verified() {
    let mut vp = vp_world(true, true);
    let mut rc = receipt(&vp.seller, vp.listing, &vp.buyer);
    rc.purchased_at += 1;
    assert_eq!(purchase_of(&mut vp, &rc), "receipt_invalid");
}

#[test]
fn review_citing_someone_elses_receipt_not_verified() {
    let mut vp = vp_world(true, true);
    let other_buyer = Author::from_label("test/other-buyer");
    let rc = receipt(&vp.seller, vp.listing, &other_buyer);
    assert_eq!(purchase_of(&mut vp, &rc), "receipt_invalid");
}

#[test]
fn receipt_for_unknown_listing_not_verified() {
    let mut vp = vp_world(true, true);
    let rc = receipt(&vp.seller, perch_core::RecordId([9; 32]), &vp.buyer);
    assert_eq!(purchase_of(&mut vp, &rc), "receipt_invalid");
}

#[test]
fn receipt_for_other_product_not_verified() {
    let mut vp = vp_world(true, true);
    let rc = receipt(&vp.seller, vp.other_listing, &vp.buyer);
    let _ = &vp.other;
    assert_eq!(purchase_of(&mut vp, &rc), "wrong_product");
}

#[test]
fn unverified_seller_receipt_not_verified() {
    let mut vp = vp_world(false, true);
    let rc = receipt(&vp.seller, vp.listing, &vp.buyer);
    assert_eq!(purchase_of(&mut vp, &rc), "seller_unverified");
}

#[test]
fn unverified_buyer_not_verified_unless_policy_allows() {
    let mut vp = vp_world(true, false);
    let rc = receipt(&vp.seller, vp.listing, &vp.buyer);
    assert_eq!(purchase_of(&mut vp, &rc), "buyer_unverified");
    vp.w.policy.verified_purchase_requires_verified_buyer = false;
    let pv = vp.w.views().product(vp.key.as_str()).unwrap();
    assert_eq!(pv.reviews[0].purchase, "verified");
}

#[test]
fn seller_review_with_own_receipt_not_verified() {
    let mut vp = vp_world(true, true);
    let mut alt = vp.w.key("alt");
    vp.w.verify(&alt, "seller");
    let rc = receipt(&vp.seller, vp.listing, &alt);
    let key = vp.key.clone();
    vp.w.review(&mut alt, &key, 5, Some(&rc));
    let pv = vp.w.views().product(key.as_str()).unwrap();
    let r = &pv.reviews[0];
    assert_eq!((r.purchase, r.excluded_reason), ("self", Some("seller")));
    assert_eq!(pv.ratings.count, 0);
}

#[test]
fn receipt_survives_listing_retract_and_replacement() {
    let mut vp = vp_world(true, true);
    let rc = receipt(&vp.seller, vp.listing, &vp.buyer);
    let key = vp.key.clone();
    let l2 = vp.w.put_r(&mut vp.seller, "listing", listing_body(&key, "https://shop.example/p", 1), Some(vp.listing));
    vp.w.retract(&mut vp.seller, l2.id());
    assert_eq!(purchase_of(&mut vp, &rc), "verified");
}

// ---------------------------------------------------------------------------
// Review counting (9.3)
// ---------------------------------------------------------------------------

#[test]
fn latest_review_per_author_and_person() {
    let mut w = World::new();
    let mut s = w.key("seller");
    let key = w.product(&mut s, "P", G1);
    let mut a = w.key("a");
    let mut a2 = w.key("a2");
    w.verify(&a, "human");
    w.verify(&a2, "human");
    w.review(&mut a, &key, 1, None);
    w.review(&mut a, &key, 2, None);
    w.review(&mut a2, &key, 5, None);
    let pv = w.views().product(key.as_str()).unwrap();
    assert_eq!(pv.ratings.count, 1);
    assert_eq!(pv.ratings.mean.unwrap().text(), "5");
    assert_eq!(pv.ratings.excluded.superseded, 2);
}

#[test]
fn replaced_review_uses_latest_valid_version() {
    let mut w = World::new();
    let mut s = w.key("seller");
    let key = w.product(&mut s, "P", G1);
    let other = w.product(&mut s, "Q", G2);
    let mut a = w.key("a");
    let r1 = w.review(&mut a, &key, 2, None);
    let r2 = w.put_r(&mut a, "review", json!({ "product": key.as_str(), "rating": 4, "text": "Better." }), Some(r1.id()));
    // A third version that moves the review to another product is invalid.
    w.put_r(&mut a, "review", json!({ "product": other.as_str(), "rating": 1, "text": "Moved." }), Some(r2.id()));
    let pv = w.views().product(key.as_str()).unwrap();
    assert_eq!((pv.reviews[0].id, pv.reviews[0].latest, pv.reviews[0].rating), (r1.id(), r2.id(), Some(4)));
    assert!(w.views().product(other.as_str()).unwrap().reviews.is_empty());
    let inv = perch_views::invalid_records(&w.store);
    assert_eq!(inv.len(), 1);
    assert_eq!(inv[0].1, "fixed:product");
}

#[test]
fn retract_withdraws_whole_chain_and_is_final() {
    let mut w = World::new();
    let mut s = w.key("seller");
    let key = w.product(&mut s, "P", G1);
    let mut a = w.key("a");
    let r1 = w.review(&mut a, &key, 2, None);
    let r2 = w.put_r(&mut a, "review", json!({ "product": key.as_str(), "rating": 3, "text": "v2" }), Some(r1.id()));
    w.retract(&mut a, r1.id());
    w.put_r(&mut a, "review", json!({ "product": key.as_str(), "rating": 5, "text": "v3 after retract" }), Some(r2.id()));
    let pv = w.views().product(key.as_str()).unwrap();
    let r = &pv.reviews[0];
    assert_eq!((r.status, r.latest, r.counted), (Status::Retracted, r2.id(), false));
    assert_eq!(pv.ratings.excluded.retracted, 1);
}

#[test]
fn retract_by_other_author_or_lower_seq_is_invalid() {
    let mut w = World::new();
    let mut s = w.key("seller");
    let key = w.product(&mut s, "P", G1);
    let mut a = w.key("a");
    let mut v = w.key("vandal");
    let r = w.review(&mut a, &key, 2, None);
    let t = w.retract(&mut v, r.id());
    let inv = perch_views::invalid_records(&w.store);
    assert_eq!(inv, vec![(t.id(), "retract:author".to_owned())]);
    assert_eq!(w.views().product(key.as_str()).unwrap().reviews[0].status, Status::Visible);
}

// ---------------------------------------------------------------------------
// Facts (10.4)
// ---------------------------------------------------------------------------

#[test]
fn verified_person_beats_many_unverified_keys() {
    let mut w = World::new();
    let mut v = w.key("verified");
    w.verify(&v, "v");
    w.put(&mut v, "product", json!({ "name": "Right Name", "brand": "B", "gtin": G1 }));
    for i in 0..5 {
        let mut k = w.key(&format!("u{i}"));
        w.put(&mut k, "product", json!({ "name": "Wrong Name", "brand": "B", "gtin": G1 }));
    }
    let key = ProductKey::from_gtin(G1).unwrap();
    let pv = w.views().product(key.as_str()).unwrap();
    assert_eq!(pv.name.value, "Right Name");
    assert_eq!(pv.name.conflicts[0].value, "Wrong Name");
    assert_eq!(pv.name.conflicts[0].sources.len(), 5);
    assert_eq!(pv.brand.sources.len(), 6);
}

#[test]
fn fact_ties_break_by_value_and_persons_count_once() {
    let mut w = World::new();
    let mut a = w.key("a");
    let mut a2 = w.key("a2");
    let mut b = w.key("b");
    w.verify(&a, "same");
    w.verify(&a2, "same");
    w.verify(&b, "other");
    w.put(&mut a, "product", json!({ "name": "Zeta", "brand": "B", "gtin": G1 }));
    w.put(&mut a2, "product", json!({ "name": "Zeta", "brand": "B", "gtin": G1 }));
    w.put(&mut b, "product", json!({ "name": "Alpha", "brand": "B", "gtin": G1 }));
    let key = ProductKey::from_gtin(G1).unwrap();
    let pv = w.views().product(key.as_str()).unwrap();
    // 1 verified person each: tie broken alphabetically.
    assert_eq!(pv.name.value, "Alpha");
}

#[test]
fn hidden_product_record_is_not_a_source_when_visible_ones_exist() {
    let mut w = World::new();
    let mut a = w.key("a");
    let mut b = w.key("b");
    w.verify(&a, "a");
    w.verify(&b, "b");
    let p = w.put(&mut a, "product", json!({ "name": "Spam Name", "brand": "B", "gtin": G1 }));
    w.put(&mut b, "product", json!({ "name": "Good Name", "brand": "B", "gtin": G1 }));
    for mut f in w.flaggers(5, "f") {
        w.flag(&mut f, p.id());
    }
    let key = ProductKey::from_gtin(G1).unwrap();
    let pv = w.views().product(key.as_str()).unwrap();
    assert_eq!(pv.name.value, "Good Name");
    assert!(pv.name.conflicts.is_empty());
    assert_eq!(pv.excluded_sources.hidden, 1);
}

// ---------------------------------------------------------------------------
// Identity, revocation, retirement, forks, time (7.2, 7.3, 5.2)
// ---------------------------------------------------------------------------

#[test]
fn expired_attestation_not_verified() {
    let mut w = World::new();
    let k = w.key("k");
    let mut att = w.attestor.clone();
    w.attest(&mut att, &k, "k", NOW);
    w.attestor = att;
    assert!(!w.views().verified_author(&k.id()), "expires must be > now");
}

#[test]
fn retracted_attestation_not_verified() {
    let mut w = World::new();
    let k = w.key("k");
    let a = w.verify(&k, "k");
    let mut att = w.attestor.clone();
    w.retract(&mut att, a.id());
    w.attestor = att;
    assert!(!w.views().verified_author(&k.id()));
}

#[test]
fn untrusted_and_wrong_kind_attestors_do_not_verify() {
    let mut w = World::new();
    let k = w.key("k");
    let mut rogue = w.key("rogue");
    w.attest(&mut rogue, &k, "k", EXP);
    assert!(!w.views().verified_author(&k.id()));
    w.policy.trusted_attestors.push(perch_views::TrustedAttestor {
        author: rogue.id(),
        kinds: vec!["attest.domain".into()],
        not_after_seq: None,
    });
    assert!(!w.views().verified_author(&k.id()), "trusted for domains only");
}

#[test]
fn not_after_seq_cuts_off_attestor() {
    let mut w = World::new();
    let k1 = w.key("k1");
    let k2 = w.key("k2");
    let a1 = w.verify(&k1, "k1");
    w.verify(&k2, "k2");
    w.policy.trusted_attestors[0].not_after_seq = Some(a1.seq());
    let v = w.views();
    assert!(v.verified_author(&k1.id()));
    assert!(!v.verified_author(&k2.id()));
}

#[test]
fn revoke_ignores_later_records_and_unrelated_revokes_are_ignored() {
    let mut w = World::new();
    let mut s = w.key("seller");
    let key = w.product(&mut s, "P", G1);
    let mut bob = w.key("bob");
    w.verify(&bob, "bob");
    let honest = w.review(&mut bob, &key, 5, None);
    let mut other = bob.clone();
    // The thief writes after the honest review.
    let spam = w.review(&mut other, &key, 1, None);
    let _ = spam;
    // A revoke from an attestor that never attested bob is ignored.
    let mut b2 = Author::from_label("test/attestor-2");
    w.policy = perch_views::default_policy(NOW, &[w.attestor.id(), b2.id()]);
    w.put(&mut b2, "attest.revoke", json!({ "subject": bob.id().to_hex(), "valid_through_seq": 0 }));
    assert_eq!(w.views().author(&bob.id()).unwrap().status, AuthorStatus::Ok);
    let mut att = w.attestor.clone();
    w.put(&mut att, "attest.revoke", json!({ "subject": bob.id().to_hex(), "valid_through_seq": honest.seq() }));
    w.attestor = att;
    let v = w.views();
    let a = v.author(&bob.id()).unwrap();
    assert_eq!(a.status, AuthorStatus::Revoked);
    assert_eq!(a.revoked.unwrap().valid_through_seq, honest.seq());
    let pv = v.product(key.as_str()).unwrap();
    assert_eq!(pv.reviews.len(), 1);
    assert_eq!(pv.reviews[0].latest, honest.id());
    assert_eq!(pv.ratings.excluded.revoked, 1, "the thief's review chain is entirely above the cut");
}

#[test]
fn retire_ignores_later_records() {
    let mut w = World::new();
    let mut s = w.key("seller");
    let key = w.product(&mut s, "P", G1);
    let mut c = w.key("carol");
    w.review(&mut c, &key, 4, None);
    w.put(&mut c, "retire", json!({}));
    let mut s2 = w.key("s2");
    let key2 = w.product(&mut s2, "Q", G2);
    w.review(&mut c, &key2, 1, None);
    let v = w.views();
    assert_eq!(v.author(&c.id()).unwrap().status, AuthorStatus::Retired);
    assert_eq!(v.product(key.as_str()).unwrap().ratings.count, 1);
    assert!(v.product(key2.as_str()).unwrap().reviews.is_empty());
}

#[test]
fn forked_author_excluded_and_person_restricted() {
    let mut w = World::new();
    let mut s = w.key("seller");
    let key = w.product(&mut s, "P", G1);
    let mut m = w.key("mallory");
    let mut m2 = w.key("mallory-alt");
    w.verify(&m, "mallory");
    w.verify(&m2, "mallory");
    w.review(&mut m, &key, 1, None);
    let a = w.review(&mut m, &key, 2, None);
    let b = m.publish_conflicting(w.tick(), "profile", json!({ "name": "M", "role": "buyer" }), None);
    w.insert(&b);
    assert_eq!(perch_store::Store::fork_seq(&w.store, &m.id()), Some(a.seq()));
    let v = w.views();
    let av = v.author(&m.id()).unwrap();
    assert_eq!((av.status, av.restricted, av.fork_seq), (AuthorStatus::Forked, true, Some(a.seq())));
    assert!(v.author(&m2.id()).unwrap().restricted, "restriction follows the person");
    let pv = v.product(key.as_str()).unwrap();
    assert_eq!(pv.reviews[0].excluded_reason, Some("forked"));
    assert_eq!(pv.ratings.count, 0);
    let _ = &mut m2;
}

#[test]
fn stolen_key_fork_does_not_restrict_person_after_revoke() {
    let mut w = World::new();
    let mut k = w.key("victim");
    let k2 = w.key("victim-new");
    w.verify(&k, "victim");
    w.verify(&k2, "victim");
    let p = w.put(&mut k, "profile", json!({ "name": "V", "role": "buyer" }));
    w.put(&mut k, "profile", json!({ "name": "V2", "role": "buyer" }));
    let twin = k.publish_conflicting(w.tick(), "profile", json!({ "name": "Thief", "role": "buyer" }), None);
    w.insert(&twin);
    assert!(w.views().author(&k2.id()).unwrap().restricted);
    let mut att = w.attestor.clone();
    w.put(&mut att, "attest.revoke", json!({ "subject": k.id().to_hex(), "valid_through_seq": p.seq() }));
    w.attestor = att;
    let v = w.views();
    assert!(!v.author(&k2.id()).unwrap().restricted);
    assert_eq!(v.author(&k.id()).unwrap().status, AuthorStatus::Revoked);
}

#[test]
fn future_ts_ignored() {
    let mut w = World::new();
    let mut s = w.key("seller");
    let key = w.product(&mut s, "P", G1);
    let mut a = w.key("a");
    let r = a.publish(NOW + 600_001, "review", json!({ "product": key.as_str(), "rating": 5, "text": "Future." }), None);
    w.insert(&r);
    let v = w.views();
    assert!(v.product(key.as_str()).unwrap().reviews.is_empty());
    assert_eq!(v.record_status(&r.id()).unwrap().ignored, Some("future"));
    let mut p = w.policy.clone();
    p.now = NOW + 1;
    assert_eq!(perch_views::product_view(&w.store, &p, key.as_str()).unwrap().reviews.len(), 1);
}

// ---------------------------------------------------------------------------
// Prices and domains (7.4, 8.3, 10.5)
// ---------------------------------------------------------------------------

#[test]
fn only_domain_verified_listings_feed_prices() {
    let mut w = World::new();
    let mut s = w.key("seller");
    w.verify(&s, "s");
    w.domain(&s, "shop.example");
    let mut p = w.key("phish");
    let key = w.product(&mut s, "P", G1);
    w.listing(&mut s, &key, "https://www.shop.example/p", 1000);
    w.listing(&mut s, &key, "https://other.example/p", 1100);
    w.listing(&mut p, &key, "https://sh0p.example/p", 100);
    let pv = w.views().product(key.as_str()).unwrap();
    let eur = &pv.price_range["EUR"];
    assert_eq!((eur.min, eur.count, eur.excluded_unverified_domain), (Some(1000), 1, 2));
    let dv: Vec<bool> = pv.listings.iter().map(|l| l.domain_verified).collect();
    assert_eq!(dv.iter().filter(|x| **x).count(), 1);
    w.policy.price_requires_domain_verified = false;
    let pv = w.views().product(key.as_str()).unwrap();
    assert_eq!(pv.price_range["EUR"].min, Some(100));
}

#[test]
fn lower_median_and_currencies_kept_apart() {
    let mut w = World::new();
    w.policy.price_requires_domain_verified = false;
    let key = ProductKey::from_gtin(G1).unwrap();
    let mut s0 = w.key("s0");
    w.product(&mut s0, "P", G1);
    for (i, (amount, cur)) in [(100, "EUR"), (300, "EUR"), (200, "EUR"), (400, "EUR"), (50, "GBP")].iter().enumerate() {
        let mut s = w.key(&format!("s{i}"));
        let mut b = listing_body(&key, &format!("https://s{i}.example/p"), *amount);
        b["price"]["currency"] = json!(cur);
        w.put(&mut s, "listing", b);
    }
    let pv = w.views().product(key.as_str()).unwrap();
    let eur = &pv.price_range["EUR"];
    assert_eq!((eur.min, eur.max, eur.median, eur.count), (Some(100), Some(400), Some(200), 4));
    assert_eq!(pv.price_range["GBP"].min, Some(50));
}

// ---------------------------------------------------------------------------
// Redaction (5.3)
// ---------------------------------------------------------------------------

#[test]
fn redact_notice_only_when_body_missing() {
    let mut w = World::new();
    let mut s = w.key("seller");
    let key = w.product(&mut s, "P", G1);
    let mut a = w.key("a");
    let r1 = w.review(&mut a, &key, 3, None);
    let r2 = w.put_r(&mut a, "review", json!({ "product": key.as_str(), "rating": 1, "text": "Phone 555" }), Some(r1.id()));
    let mut node = w.key("node");
    w.put(&mut node, "redact", json!({ "target": r2.id().to_hex(), "reason": "personal_data" }));
    let v = w.views();
    let pv = v.product(key.as_str()).unwrap();
    assert_eq!(pv.reviews[0].status, Status::Visible);
    assert!(pv.reviews[0].redaction.is_none(), "body present: no notice");
    drop(v);
    perch_store::Store::remove_body(&mut w.store, &r2.id()).unwrap();
    let pv = w.views().product(key.as_str()).unwrap();
    let r = &pv.reviews[0];
    assert_eq!((r.status, r.counted, r.rating), (Status::Unavailable, false, None));
    let red = r.redaction.as_ref().unwrap();
    assert_eq!(red.reason.as_deref(), Some("personal_data"));
    assert_eq!(red.by, Some(node.id()));
}

#[test]
fn view_json_is_canonical_and_stable() {
    let (w, l) = hidden_listing_world(3);
    let a = canonical_json(&w.views().listing(&l));
    let b = canonical_json(&perch_views::listing_view(&w.store, &w.policy, &l));
    assert_eq!(a, b);
    assert!(a.starts_with("{\"appeal\":null,\"appealed\":false,"));
}

use perch_demo::Author;
