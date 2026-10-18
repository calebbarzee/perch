//! Ranking resistance (contract 7.4, 7.8): keys cannot buy ranking or prices.

use perch_tests::fixture::Log;
use perch_tests::require_sut;
use perch_tests::sut::{Store, product_view, search};
use perch_tests::world::{World, key_a, product_and_listing};
use serde_json::{Value, json};

fn store_of(w: &World, logs: &[&Log]) -> Store {
    let mut s = Store::new();
    for l in w.lines(logs) {
        s.ingest(&l);
    }
    s
}

fn hit(hits: &Value, key: &str) -> Value {
    hits.as_array()
        .and_then(|a| a.iter().find(|h| h["key"] == json!(key)).cloned())
        .unwrap_or(Value::Null)
}

#[test]
fn unverified_reviews_do_not_change_search_rating() {
    require_sut!();
    let mut w = World::new();
    let mut maker = Log::new("rank-maker");
    let mut seller = w.seller("rank-seller", "shop.example");
    product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let mut honest = w.verified("rank-honest", "honest");
    honest.review(&key_a(), 2, None);
    let base = store_of(&w, &[&maker, &seller, &honest]);
    let before = hit(&search(&base, &w.policy(), "Widget"), &key_a());

    let mut sybils: Vec<Log> = (0..100).map(|i| Log::new(&format!("rank-sybil-{i}"))).collect();
    for s in sybils.iter_mut() {
        s.review(&key_a(), 5, None);
    }
    let mut logs: Vec<&Log> = vec![&maker, &seller, &honest];
    logs.extend(sybils.iter());
    let s = store_of(&w, &logs);
    let after = hit(&search(&s, &w.policy(), "Widget"), &key_a());
    assert_eq!(after["rating_mean"], before["rating_mean"]);
    assert_eq!(after["rating_count"], json!(1));
    assert_eq!(after["score"], before["score"], "score must not move with free keys");
}

#[test]
fn many_keys_one_person_count_once() {
    require_sut!();
    let mut w = World::new();
    let mut maker = Log::new("rank1-maker");
    let mut seller = w.seller("rank1-seller", "shop.example");
    product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let mut keys: Vec<Log> = (0..10).map(|i| w.verified(&format!("rank1-k{i}"), "one-human")).collect();
    for k in keys.iter_mut() {
        k.review(&key_a(), 5, None);
    }
    let mut logs: Vec<&Log> = vec![&maker, &seller];
    logs.extend(keys.iter());
    let s = store_of(&w, &logs);
    let pv = product_view(&s, &w.policy(), &key_a());
    assert_eq!(pv["ratings"]["verified_identity"]["count"], json!(1));
    assert_eq!(hit(&search(&s, &w.policy(), "Widget"), &key_a())["rating_count"], json!(1));
}

#[test]
fn phishing_listing_excluded_from_price_range() {
    require_sut!();
    let mut w = World::new();
    let mut maker = Log::new("rank-phish-maker");
    let mut seller = w.seller("rank-phish-seller", "shop.example");
    product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    // Verified scammer without a domain attestation lists at 1 cent.
    let mut scam = w.verified("rank-phish-scam", "scam");
    scam.listing(&key_a(), "https://shop-example.evil/widget", 1);
    let s = store_of(&w, &[&maker, &seller, &scam]);
    let p = w.policy();
    let pv = product_view(&s, &p, &key_a());
    assert_eq!(pv["price_range"]["EUR"]["min"], json!(1000));
    let h = hit(&search(&s, &p, "Widget"), &key_a());
    assert_eq!(h["min_price"]["amount"], json!(1000));
    let scam_view = pv["listings"].as_array().unwrap().iter().find(|l| l["seller"] == json!(scam.author())).cloned().unwrap();
    assert_eq!(scam_view["domain_verified"], json!(false));
}

#[test]
fn userinfo_url_not_domain_verified() {
    require_sut!();
    // https://shop.example@evil.example/ has host evil.example (WHATWG).
    let mut w = World::new();
    let mut maker = Log::new("rank-ui-maker");
    let mut seller = w.seller("rank-ui-seller", "shop.example");
    product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let mut scam = w.verified("rank-ui-scam", "ui-scam");
    w.doma.attest_domain(&scam.author(), "evil.example", perch_tests::world::EXPIRES);
    let l = scam.listing(&key_a(), "https://shop.example@evil.example/widget", 1);
    let s = store_of(&w, &[&maker, &seller, &scam]);
    let pv = product_view(&s, &w.policy(), &key_a());
    let v = pv["listings"].as_array().unwrap().iter().find(|x| x["id"] == json!(l.id)).cloned();
    // Userinfo makes the URL invalid (4.5): the listing is ignored or at least not priced.
    assert!(v.is_none() || v.unwrap()["domain_verified"] == json!(false));
    assert_eq!(pv["price_range"]["EUR"]["min"], json!(1000));
}

#[test]
fn lookalike_hosts_not_domain_verified() {
    require_sut!();
    // Seller is attested for shop.example. Only that host and its subdomains match (7.4).
    let mut w = World::new();
    let mut maker = Log::new("rank-la-maker");
    let mut seller = w.seller("rank-la-seller", "shop.example");
    product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let sub = seller.listing(&key_a(), "https://www.shop.example/w", 1001);
    let upper = seller.listing(&key_a(), "https://WWW.Shop.Example/w", 1002);
    let suffix = seller.listing(&key_a(), "https://xshop.example/w", 2);
    let prefix = seller.listing(&key_a(), "https://shop.example.evil.example/w", 3);
    let s = store_of(&w, &[&maker, &seller]);
    let pv = product_view(&s, &w.policy(), &key_a());
    let dv = |id: &str| {
        pv["listings"].as_array().unwrap().iter().find(|x| x["id"] == json!(id)).map(|x| x["domain_verified"].clone())
    };
    assert_eq!(dv(&sub.id), Some(json!(true)), "subdomain matches");
    assert_eq!(dv(&upper.id), Some(json!(true)), "hosts compare case-insensitively after parsing");
    assert_eq!(dv(&suffix.id), Some(json!(false)), "xshop.example is not under shop.example");
    assert_eq!(dv(&prefix.id), Some(json!(false)), "shop.example.evil.example is not under shop.example");
    assert_eq!(pv["price_range"]["EUR"]["min"], json!(1000));
}
