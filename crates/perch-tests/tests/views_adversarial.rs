//! Further adversarial checks on views (contract 7, docs/views.md).

use perch_tests::fixture::{Log, Opts};
use perch_tests::require_sut;
use perch_tests::sut::{Store, author_view, listing_view, product_view, search};
use perch_tests::world::{GTIN_A, World, key_a, product_and_listing};
use serde_json::{Value, json};

fn store_of(w: &World, logs: &[&Log]) -> Store {
    let mut s = Store::new();
    for l in w.lines(logs) {
        s.ingest(&l);
    }
    s
}

fn all<'a>(base: &[&'a Log], extra: &'a [Log]) -> Vec<&'a Log> {
    let mut v = base.to_vec();
    v.extend(extra.iter());
    v
}

#[test]
fn verified_person_outranks_sybil_product_facts() {
    require_sut!(Views);
    let mut w = World::new();
    let mut honest = w.verified("va-fact-honest", "fact-honest");
    honest.product_gtin("Widget", "Acme", GTIN_A);
    let mut sybils: Vec<Log> = (0..30).map(|i| Log::new(&format!("va-fact-sybil-{i}"))).collect();
    for s in sybils.iter_mut() {
        s.product_gtin("SCAM - call this number", "Acme", GTIN_A);
    }
    let s = store_of(&w, &all(&[&honest], &sybils));
    let pv = product_view(&s, &w.policy(), &key_a());
    assert_eq!(pv["name"]["value"], json!("Widget"), "one verified person beats 30 free keys");
    assert_eq!(pv["name"]["conflicts"][0]["value"], json!("SCAM - call this number"), "conflict stays visible");
}

#[test]
fn one_person_many_keys_count_once_in_facts() {
    require_sut!(Views);
    let mut w = World::new();
    let mut one: Vec<Log> = (0..5).map(|i| w.verified(&format!("va-f1-{i}"), "same-person")).collect();
    for k in one.iter_mut() {
        k.product_gtin("Bad name", "Acme", GTIN_A);
    }
    let mut two: Vec<Log> = (0..2).map(|i| w.verified(&format!("va-f2-{i}"), &format!("other-{i}"))).collect();
    for k in two.iter_mut() {
        k.product_gtin("Good name", "Acme", GTIN_A);
    }
    let mut logs = all(&[], &one);
    logs.extend(two.iter());
    let s = store_of(&w, &logs);
    assert_eq!(product_view(&s, &w.policy(), &key_a())["name"]["value"], json!("Good name"));
}

#[test]
fn sybil_images_marked_unverified() {
    require_sut!(Views);
    let mut w = World::new();
    let mut honest = w.verified("va-img-honest", "img-honest");
    honest.product_gtin("Widget", "Acme", GTIN_A);
    let mut sybil = Log::new("va-img-sybil");
    sybil.add("product", json!({ "name": "Widget", "brand": "Acme", "gtin": GTIN_A, "images": [{ "url": "https://tracker.example/pixel.png" }] }));
    let s = store_of(&w, &[&honest, &sybil]);
    let pv = product_view(&s, &w.policy(), &key_a());
    // The honest chain is visible, so both are sources; the image keeps its unverified mark.
    let img = pv["images"].as_array().unwrap().iter().find(|i| i["url"] == json!("https://tracker.example/pixel.png")).cloned();
    let img = img.expect("both chains are visible sources, so the image is listed");
    assert_eq!(img["author_verified"], json!(false), "client must not auto-load it (contract 10)");
}

#[test]
fn forked_reviewer_excluded_from_ratings() {
    require_sut!(Views);
    let mut w = World::new();
    let mut maker = Log::new("va-fork-maker");
    let mut seller = w.seller("va-fork-seller", "shop.example");
    product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let mut r = w.verified("va-fork-rev", "fork-rev");
    r.profile("R", "buyer");
    r.review(&key_a(), 5, None); // seq 2, below the fork
    r.profile("R2", "buyer"); // seq 3
    let alt = r.fork_at(3, "profile", json!({ "name": "X", "role": "buyer" }));
    let mut s = store_of(&w, &[&maker, &seller, &r]);
    s.ingest(&alt.wire());
    let p = w.policy();
    let ratings = &product_view(&s, &p, &key_a())["ratings"];
    assert_eq!(ratings["count"], json!(0));
    assert_eq!(ratings["excluded"]["forked"], json!(1));
    assert_eq!(author_view(&s, &p, &r.author())["status"], json!("forked"));
}

#[test]
fn forked_flagger_flags_not_counted() {
    require_sut!(Views);
    let mut w = World::new();
    let mut maker = Log::new("va-ff-maker");
    let mut seller = w.seller("va-ff-seller", "shop.example");
    let (_p, l) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let mut crowd = w.crowd("va-ff-f", 3);
    for c in crowd.iter_mut() {
        c.flag(&l.id, "fraud");
        c.profile("x", "buyer");
    }
    let alt = crowd[0].fork_at(2, "profile", json!({ "name": "fork", "role": "buyer" }));
    let mut s = store_of(&w, &all(&[&maker, &seller], &crowd));
    let p = w.policy();
    assert_eq!(listing_view(&s, &p, &l.id)["status"], json!("hidden"), "control: 3 flags hide");
    s.ingest(&alt.wire());
    let v = listing_view(&s, &p, &l.id);
    assert_eq!(v["flags"]["counted"], json!(2));
    assert_eq!(v["status"], json!("visible"));
}

#[test]
fn records_after_retire_ignored() {
    require_sut!(Views);
    let mut w = World::new();
    let mut maker = Log::new("va-ret-maker");
    let mut seller = w.seller("va-ret-seller", "shop.example");
    let (_p, l1) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    seller.add("retire", json!({}));
    let l2 = seller.listing(&key_a(), "https://shop.example/after", 1);
    let s = store_of(&w, &[&maker, &seller]);
    let p = w.policy();
    assert_eq!(author_view(&s, &p, &seller.author())["status"], json!("retired"));
    assert!(!listing_view(&s, &p, &l1.id).is_null());
    let pv = product_view(&s, &p, &key_a());
    let ids: Vec<&Value> = pv["listings"].as_array().unwrap().iter().map(|x| &x["id"]).collect();
    assert!(!ids.contains(&&json!(l2.id)), "listing after retire must be ignored");
    assert_eq!(pv["price_range"]["EUR"]["min"], json!(1000));
}

#[test]
fn retire_successor_inherits_nothing() {
    require_sut!(Views);
    let mut w = World::new();
    let mut old = w.verified("va-succ-old", "succ-old");
    old.profile("Old", "seller");
    let mut thief = Log::new("va-succ-thief");
    thief.profile("New", "seller");
    old.add("retire", json!({ "successor": thief.author() }));
    let s = store_of(&w, &[&old, &thief]);
    assert!(author_view(&s, &w.policy(), &thief.author())["verified"].is_null(), "no verification transfers (4.5)");
}

#[test]
fn future_flags_do_not_count() {
    require_sut!(Views);
    let mut w = World::new();
    let mut maker = Log::new("va-futf-maker");
    let mut seller = w.seller("va-futf-seller", "shop.example");
    let (_p, l) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let mut crowd = w.crowd("va-futf-f", 3);
    for c in crowd.iter_mut() {
        c.add_with(
            "flag",
            json!({ "target": l.id, "reason": "fraud" }),
            Opts { ts: Some(perch_tests::world::NOW + 3_600_000), ..Default::default() },
        );
    }
    let s = store_of(&w, &all(&[&maker, &seller], &crowd));
    assert_eq!(listing_view(&s, &w.policy(), &l.id)["status"], json!("visible"));
}

#[test]
fn mean_rounds_half_to_even() {
    require_sut!(Views);
    // 8 verified reviews summing to 17: 17/8 = 2.125 → 2.12 (half to even).
    // 8 reviews summing to 19: 2.375 → 2.38.
    for (ratings, expected) in [([3, 3, 3, 2, 2, 2, 1, 1], 2.12), ([3, 3, 3, 3, 2, 2, 2, 1], 2.38)] {
        let mut w = World::new();
        let mut maker = Log::new("va-mean-maker");
        let mut seller = w.seller("va-mean-seller", "shop.example");
        product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
        let mut crowd = w.crowd(&format!("va-mean-{expected}-"), 8);
        for (c, r) in crowd.iter_mut().zip(ratings) {
            c.review(&key_a(), r, None);
        }
        let s = store_of(&w, &all(&[&maker, &seller], &crowd));
        let rs = &product_view(&s, &w.policy(), &key_a())["ratings"];
        assert_eq!(rs["verified_identity"]["mean"], json!(expected), "ratings {ratings:?}");
    }
}

#[test]
fn views_are_independent_of_ingest_order() {
    require_sut!(Views);
    // Same records in reverse order (gaps resolve) give byte-identical views.
    let mut w = World::new();
    let mut maker = Log::new("va-ord-maker");
    let mut seller = w.seller("va-ord-seller", "shop.example");
    let (_p, l) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let mut crowd = w.crowd("va-ord-f", 4);
    for (i, c) in crowd.iter_mut().enumerate() {
        c.flag(&l.id, "fraud");
        c.review(&key_a(), 1 + i as i64, None);
    }
    let logs = all(&[&maker, &seller], &crowd);
    let fwd = store_of(&w, &logs);
    let mut rev = Store::new();
    let lines = w.lines(&logs);
    for _ in 0..2 {
        for line in lines.iter().rev() {
            rev.ingest(line);
        }
    }
    let p = w.policy();
    assert_eq!(product_view(&fwd, &p, &key_a()), product_view(&rev, &p, &key_a()));
    assert_eq!(search(&fwd, &p, "widget"), search(&rev, &p, "widget"));
}

#[test]
fn policy_hash_ignores_order_and_nulls() {
    require_sut!(Views);
    let w = World::new();
    let mut p1 = w.policy();
    let mut p2 = w.policy();
    let ta = p2["trusted_attestors"].as_array_mut().unwrap();
    ta.reverse();
    for t in ta.iter_mut() {
        t.as_object_mut().unwrap().remove("not_after_seq");
        t["kinds"].as_array_mut().unwrap().reverse();
    }
    p2["now"] = json!(1);
    let h = |p: &Value| perch_views::policy_hash(&perch_views::Policy::from_value(p.clone()).unwrap());
    assert_eq!(h(&p1), h(&p2));
    p1["flag_threshold"] = json!(4);
    assert_ne!(h(&p1), h(&p2), "a rule change must change the hash");
}

#[test]
fn invalid_policy_rejected() {
    require_sut!(Views);
    let w = World::new();
    for (field, bad) in [
        ("flag_threshold", json!(0)),
        ("trusted_attestors", json!([{ "author": "AB", "kinds": ["attest.identity"] }])),
        ("trusted_attestors", json!([{ "author": "a".repeat(64), "kinds": ["profile"] }])),
        ("now", json!(-1)),
    ] {
        let mut p = w.policy();
        p[field] = bad.clone();
        assert!(perch_views::Policy::from_value(p).is_err(), "{field} = {bad} must be rejected");
    }
}

#[test]
fn seller_alt_key_review_excluded_from_ratings() {
    require_sut!(Views);
    // A seller's second key (same person, no listing) reviews its own product.
    let mut w = World::new();
    let mut maker = Log::new("va-salt-maker");
    let mut seller = w.seller("va-salt-seller", "shop.example");
    product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let mut alt = w.verified("va-salt-alt", "va-salt-seller");
    alt.review(&key_a(), 5, None);
    let s = store_of(&w, &[&maker, &seller, &alt]);
    let rs = &product_view(&s, &w.policy(), &key_a())["ratings"];
    assert_eq!(rs["verified_identity"]["count"], json!(0));
    assert_eq!(rs["excluded"]["seller"], json!(1));
}
