//! Moderation (contract 7.6).

use perch_tests::fixture::Log;
use perch_tests::require_sut;
use perch_tests::sut::{Store, listing_view, product_view};
use perch_tests::world::{World, key_a, product_and_listing};
use serde_json::{Value, json};

fn store_of(w: &World, logs: &[&Log]) -> Store {
    let mut s = Store::new();
    for l in w.lines(logs) {
        s.ingest(&l);
    }
    s
}

fn setup(tag: &str) -> (World, Log, Log, String) {
    let mut w = World::new();
    let mut maker = Log::new(&format!("{tag}-maker"));
    let mut seller = w.seller(&format!("{tag}-seller"), "shop.example");
    let (_p, l) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    (w, maker, seller, l.id)
}

fn all<'a>(base: &[&'a Log], extra: &'a [Log]) -> Vec<&'a Log> {
    let mut v = base.to_vec();
    v.extend(extra.iter());
    v
}

#[test]
fn verified_flags_hide_at_threshold() {
    require_sut!();
    let (mut w, maker, seller, lid) = setup("mod-thr");
    let mut crowd = w.crowd("mod-thr-f", 3);
    crowd[0].flag(&lid, "fraud");
    crowd[1].flag(&lid, "counterfeit");
    let s = store_of(&w, &all(&[&maker, &seller], &crowd));
    let v = listing_view(&s, &w.policy(), &lid);
    assert_eq!(v["status"], json!("visible"), "2 < threshold 3");
    assert_eq!(v["flags"]["counted"], json!(2));
    crowd[2].flag(&lid, "fraud");
    let s = store_of(&w, &all(&[&maker, &seller], &crowd));
    let v = listing_view(&s, &w.policy(), &lid);
    assert_eq!(v["status"], json!("hidden"));
    assert_eq!(v["flags"]["reasons"]["fraud"], json!(2));
}

#[test]
fn seller_self_flag_not_counted() {
    require_sut!();
    let (mut w, maker, mut seller, lid) = setup("mod-self");
    let mut crowd = w.crowd("mod-self-f", 2);
    for c in crowd.iter_mut() {
        c.flag(&lid, "fraud");
    }
    seller.flag(&lid, "fraud");
    let s = store_of(&w, &all(&[&maker, &seller], &crowd));
    let v = listing_view(&s, &w.policy(), &lid);
    assert_eq!(v["flags"]["counted"], json!(2));
    assert_eq!(v["status"], json!("visible"));
}

#[test]
fn seller_other_key_same_person_flag_not_counted() {
    require_sut!();
    // The seller's second key belongs to the same person; flags by the target's
    // author never count, and that person is the target's author.
    let (mut w, maker, seller, lid) = setup("mod-self2");
    let mut alt = w.verified("mod-self2-alt", "mod-self2-seller");
    alt.flag(&lid, "fraud");
    let mut crowd = w.crowd("mod-self2-f", 2);
    for c in crowd.iter_mut() {
        c.flag(&lid, "fraud");
    }
    let mut logs = all(&[&maker, &seller], &crowd);
    logs.push(&alt);
    let s = store_of(&w, &logs);
    let v = listing_view(&s, &w.policy(), &lid);
    assert_eq!(v["status"], json!("visible"), "seller's own other key must not complete the threshold: {v}");
}

#[test]
fn unverified_sybil_flags_not_counted() {
    require_sut!();
    let (w, maker, seller, lid) = setup("mod-sybil");
    let mut sybils: Vec<Log> = (0..50).map(|i| Log::new(&format!("mod-sybil-{i}"))).collect();
    for s in sybils.iter_mut() {
        s.flag(&lid, "fraud");
    }
    let s = store_of(&w, &all(&[&maker, &seller], &sybils));
    let v = listing_view(&s, &w.policy(), &lid);
    assert_eq!(v["status"], json!("visible"));
    assert_eq!(v["flags"]["counted"], json!(0));
    assert_eq!(v["flags"]["count"], json!(50), "uncounted flags are still visible");
}

#[test]
fn one_person_many_keys_flags_count_once() {
    require_sut!();
    let (mut w, maker, seller, lid) = setup("mod-person");
    let mut keys: Vec<Log> = (0..5).map(|i| w.verified(&format!("mod-person-k{i}"), "same-human")).collect();
    for k in keys.iter_mut() {
        k.flag(&lid, "fraud");
    }
    let s = store_of(&w, &all(&[&maker, &seller], &keys));
    let v = listing_view(&s, &w.policy(), &lid);
    assert_eq!(v["flags"]["counted"], json!(1));
    assert_eq!(v["status"], json!("visible"));
}

#[test]
fn flag_retraction_unhides() {
    require_sut!();
    let (mut w, maker, seller, lid) = setup("mod-unhide");
    let mut crowd = w.crowd("mod-unhide-f", 3);
    let f0 = crowd[0].flag(&lid, "fraud");
    crowd[1].flag(&lid, "fraud");
    crowd[2].flag(&lid, "fraud");
    crowd[0].retract(&f0.id);
    let s = store_of(&w, &all(&[&maker, &seller], &crowd));
    assert_eq!(listing_view(&s, &w.policy(), &lid)["status"], json!("visible"));
}

#[test]
fn appeal_alone_does_not_unhide() {
    require_sut!();
    let (mut w, maker, mut seller, lid) = setup("mod-appeal");
    let mut crowd = w.crowd("mod-appeal-f", 3);
    for c in crowd.iter_mut() {
        c.flag(&lid, "fraud");
    }
    seller.add("appeal", json!({ "target": lid, "note": "Genuine product." }));
    let s = store_of(&w, &all(&[&maker, &seller], &crowd));
    let v = listing_view(&s, &w.policy(), &lid);
    assert_eq!(v["status"], json!("hidden"));
    assert_eq!(v["appealed"], json!(true));
}

#[test]
fn appeal_by_non_author_ignored() {
    require_sut!();
    let (mut w, maker, seller, lid) = setup("mod-appeal2");
    let mut other = w.verified("mod-appeal2-other", "other");
    other.add("appeal", json!({ "target": lid, "note": "Not mine." }));
    let s = store_of(&w, &[&maker, &seller, &other]);
    assert_eq!(listing_view(&s, &w.policy(), &lid)["appealed"], json!(false));
}

#[test]
fn product_seller_flags_on_review_not_counted() {
    require_sut!();
    // 5 verified sellers of product A (review threshold 5) flag a negative review.
    let (mut w, mut maker, seller, _lid) = setup("mod-revflag");
    let mut buyer = w.verified("mod-revflag-buyer", "rb");
    let rev = buyer.review(&key_a(), 1, None);
    let mut rivals: Vec<Log> = (0..5)
        .map(|i| {
            let mut s = w.seller(&format!("mod-revflag-s{i}"), &format!("s{i}.example"));
            s.listing(&key_a(), &format!("https://s{i}.example/w"), 900);
            s
        })
        .collect();
    for r in rivals.iter_mut() {
        r.flag(&rev.id, "misleading");
    }
    let _ = &mut maker;
    let s = store_of(&w, &all(&[&maker, &seller, &buyer], &rivals));
    let pv = product_view(&s, &w.policy(), &key_a());
    let rv = find(&pv["reviews"], &rev.id);
    assert_eq!(rv["flags"]["counted"], json!(0));
    assert_eq!(rv["status"], json!("visible"));
}

#[test]
fn review_hidden_at_its_threshold_still_counts_in_ratings() {
    require_sut!();
    let (mut w, maker, seller, _lid) = setup("mod-revhide");
    let mut buyer = w.verified("mod-revhide-buyer", "rhb");
    let rev = buyer.review(&key_a(), 1, None);
    let mut crowd = w.crowd("mod-revhide-f", 5);
    for c in crowd.iter_mut() {
        c.flag(&rev.id, "spam");
    }
    let s = store_of(&w, &all(&[&maker, &seller, &buyer], &crowd));
    let pv = product_view(&s, &w.policy(), &key_a());
    assert_eq!(find(&pv["reviews"], &rev.id)["status"], json!("hidden"), "review threshold 5");
    assert_eq!(pv["ratings"]["verified_identity"]["count"], json!(1), "hiding is display-only");
}

#[test]
fn flags_on_flags_hide_nothing() {
    require_sut!();
    let (mut w, maker, seller, lid) = setup("mod-ff");
    let mut crowd = w.crowd("mod-ff-f", 3);
    let flags: Vec<String> = crowd.iter_mut().map(|c| c.flag(&lid, "fraud").id).collect();
    let mut counter = w.crowd("mod-ff-c", 5);
    for c in counter.iter_mut() {
        for f in &flags {
            c.flag(f, "spam");
        }
    }
    let mut logs = all(&[&maker, &seller], &crowd);
    logs.extend(counter.iter());
    let s = store_of(&w, &logs);
    assert_eq!(listing_view(&s, &w.policy(), &lid)["status"], json!("hidden"), "flagged flags still count");
}

#[test]
fn restriction_requires_distinct_flaggers() {
    require_sut!();
    // 3 colluders hide 3 listings: hidden ≥ 3 but distinct flaggers 3 < 9: not restricted.
    let (mut w, mut maker, mut seller, l0) = setup("mod-restrict");
    let l1 = seller.listing(&key_a(), "https://shop.example/w1", 1001).id;
    let l2 = seller.listing(&key_a(), "https://shop.example/w2", 1002).id;
    let _ = &mut maker;
    let mut crowd = w.crowd("mod-restrict-f", 3);
    for c in crowd.iter_mut() {
        for l in [&l0, &l1, &l2] {
            c.flag(l, "fraud");
        }
    }
    let s = store_of(&w, &all(&[&maker, &seller], &crowd));
    let p = w.policy();
    assert_eq!(listing_view(&s, &p, &l0)["status"], json!("hidden"));
    assert_eq!(listing_view(&s, &p, &l0)["seller_restricted"], json!(false));

    // 9 distinct persons across the 3 hidden listings: restricted.
    let mut more = w.crowd("mod-restrict-g", 6);
    for (i, c) in more.iter_mut().enumerate() {
        c.flag([&l0, &l1, &l2][i % 3], "fraud");
    }
    let mut logs = all(&[&maker, &seller], &crowd);
    logs.extend(more.iter());
    let s = store_of(&w, &logs);
    assert_eq!(listing_view(&s, &p, &l1)["seller_restricted"], json!(true));
}

#[test]
fn restriction_not_diluted_by_dummy_listings() {
    require_sut!();
    let (mut w, maker, mut seller, l0) = setup("mod-dilute");
    let l1 = seller.listing(&key_a(), "https://shop.example/w1", 1001).id;
    let l2 = seller.listing(&key_a(), "https://shop.example/w2", 1002).id;
    for i in 0..100 {
        seller.listing(&key_a(), &format!("https://shop.example/d{i}"), 2000 + i);
    }
    // 9 distinct persons, 3 per listing: 3 hidden of 103 listings.
    let mut crowd = w.crowd("mod-dilute-f", 9);
    for (i, c) in crowd.iter_mut().enumerate() {
        c.flag([&l0, &l1, &l2][i % 3], "fraud");
    }
    let s = store_of(&w, &all(&[&maker, &seller], &crowd));
    assert_eq!(listing_view(&s, &w.policy(), &l0)["seller_restricted"], json!(true), "no ratio: dummies do not dilute");
}

#[test]
fn retracting_hidden_listings_does_not_escape_restriction() {
    require_sut!();
    // Contract 7.6: a listing counts as hidden for restriction once its counted
    // flags reach the threshold, even if the seller later retracts it.
    let (mut w, maker, mut seller, l0) = setup("mod-escape");
    let l1 = seller.listing(&key_a(), "https://shop.example/w1", 1001).id;
    let l2 = seller.listing(&key_a(), "https://shop.example/w2", 1002).id;
    let mut crowd = w.crowd("mod-escape-f", 9);
    for (i, c) in crowd.iter_mut().enumerate() {
        c.flag([&l0, &l1, &l2][i % 3], "fraud");
    }
    for l in [&l0, &l1, &l2] {
        seller.retract(l);
    }
    let l3 = seller.listing(&key_a(), "https://shop.example/w3", 999).id;
    let s = store_of(&w, &all(&[&maker, &seller], &crowd));
    assert_eq!(listing_view(&s, &w.policy(), &l3)["seller_restricted"], json!(true));
}

#[test]
fn hidden_listing_excluded_from_price_range() {
    require_sut!();
    let (mut w, maker, mut seller, l0) = setup("mod-price");
    seller.listing(&key_a(), "https://shop.example/w1", 1500);
    let mut crowd = w.crowd("mod-price-f", 3);
    for c in crowd.iter_mut() {
        c.flag(&l0, "fraud");
    }
    let s = store_of(&w, &all(&[&maker, &seller], &crowd));
    let pv = product_view(&s, &w.policy(), &key_a());
    assert_eq!(pv["price_range"]["EUR"]["min"], json!(1500), "hidden listing at 1000 must not set the minimum");
}

#[test]
fn restriction_follows_person_to_new_key() {
    require_sut!();
    let (mut w, maker, mut seller, l0) = setup("mod-follow");
    let l1 = seller.listing(&key_a(), "https://shop.example/w1", 1001).id;
    let l2 = seller.listing(&key_a(), "https://shop.example/w2", 1002).id;
    let mut crowd = w.crowd("mod-follow-f", 9);
    for (i, c) in crowd.iter_mut().enumerate() {
        c.flag([&l0, &l1, &l2][i % 3], "fraud");
    }
    // Same person, new key, new listing.
    let mut fresh = w.verified("mod-follow-new", "mod-follow-seller");
    let ln = fresh.listing(&key_a(), "https://shop.example/new", 999).id;
    let s = store_of(&w, &all(&[&maker, &seller, &fresh], &crowd));
    assert_eq!(listing_view(&s, &w.policy(), &ln)["seller_restricted"], json!(true));
}

#[test]
fn hidden_listing_still_anchors_receipts() {
    require_sut!();
    let (mut w, maker, seller, lid) = setup("mod-anchor");
    let mut buyer = w.verified("mod-anchor-buyer", "anchor-buyer");
    let rc = seller.receipt(&lid, &buyer.author(), 1, "n");
    let rev = buyer.review(&key_a(), 2, Some(rc));
    let mut crowd = w.crowd("mod-anchor-f", 3);
    for c in crowd.iter_mut() {
        c.flag(&lid, "fraud");
    }
    let s = store_of(&w, &all(&[&maker, &seller, &buyer], &crowd));
    let pv = product_view(&s, &w.policy(), &key_a());
    assert_eq!(find(&pv["listings"], &lid)["status"], json!("hidden"));
    assert_eq!(find(&pv["reviews"], &rev.id)["verified_purchase"], json!(true));
}

#[test]
fn flags_require_verified_off_counts_keys() {
    require_sut!();
    let (w, maker, seller, lid) = setup("mod-open");
    let mut anon: Vec<Log> = (0..3).map(|i| Log::new(&format!("mod-open-{i}"))).collect();
    for a in anon.iter_mut() {
        a.flag(&lid, "fraud");
    }
    let s = store_of(&w, &all(&[&maker, &seller], &anon));
    let mut p = w.policy();
    p["flags_require_verified"] = json!(false);
    assert_eq!(listing_view(&s, &p, &lid)["status"], json!("hidden"));
}

#[test]
fn product_seller_alt_key_flags_on_review_not_counted() {
    require_sut!();
    // Seller person with a second key that has no listing. Exclusion must be per
    // person, or the seller escapes the rule by flagging from the other key.
    let (mut w, maker, seller, _lid) = setup("mod-altflag");
    let mut buyer = w.verified("mod-altflag-buyer", "afb");
    let rev = buyer.review(&key_a(), 1, None);
    let mut alt = w.verified("mod-altflag-alt", "mod-altflag-seller");
    alt.flag(&rev.id, "misleading");
    let mut crowd = w.crowd("mod-altflag-f", 4);
    for c in crowd.iter_mut() {
        c.flag(&rev.id, "misleading");
    }
    let mut logs = all(&[&maker, &seller, &buyer, &alt], &crowd);
    logs.sort_by_key(|l| l.author());
    let s = store_of(&w, &logs);
    let pv = product_view(&s, &w.policy(), &key_a());
    let rv = find(&pv["reviews"], &rev.id);
    assert_eq!(rv["flags"]["counted"], json!(4), "seller's other key must not count: {rv}");
    assert_eq!(rv["status"], json!("visible"));
}

fn find(arr: &Value, id: &str) -> Value {
    arr.as_array()
        .and_then(|a| a.iter().find(|x| x["id"] == json!(id)).cloned())
        .unwrap_or(Value::Null)
}
