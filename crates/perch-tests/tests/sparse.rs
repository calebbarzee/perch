//! Light clients (contract 5.5, 8.1): a sparse store holding a product's related
//! set must give the same ProductView as the full store, also in adversarial cases.

use perch_tests::fixture::Log;
use perch_tests::require_sut;
use perch_tests::sut::{Store, product_view, related_sparse};
use perch_tests::world::{World, key_a, product_and_listing};
use serde_json::json;

fn check(w: &World, s: &Store, label: &str) {
    let p = w.policy();
    let full = product_view(s, &p, &key_a());
    let sparse = product_view(&related_sparse(s, &p, &key_a()), &p, &key_a());
    assert!(!full.is_null());
    assert_eq!(full, sparse, "{label}: sparse related-set view differs from full view");
}

fn base(tag: &str) -> (World, Log, Log, String) {
    let mut w = World::new();
    let mut maker = Log::new(&format!("{tag}-maker"));
    let mut seller = w.seller(&format!("{tag}-seller"), "shop.example");
    let (_p, l) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    (w, maker, seller, l.id)
}

fn load(w: &World, logs: &[&Log]) -> Store {
    let mut s = Store::new();
    for l in w.lines(logs) {
        s.ingest(&l);
    }
    s
}

#[test]
fn sparse_view_matches_with_forked_reviewer() {
    require_sut!(Views);
    let (mut w, maker, seller, _l) = base("sp-fork");
    let mut r = w.verified("sp-fork-rev", "sp-fork-rev");
    r.review(&key_a(), 5, None);
    r.profile("R", "buyer");
    let alt = r.fork_at(2, "profile", json!({ "name": "X", "role": "buyer" }));
    let mut s = load(&w, &[&maker, &seller, &r]);
    s.ingest(&alt.wire());
    check(&w, &s, "forked reviewer");
}

#[test]
fn sparse_view_matches_with_restricted_seller_on_other_product() {
    require_sut!(Views);
    // Restriction inputs live on another product's listings; the related set must carry them.
    let (mut w, mut maker, mut seller, _l) = base("sp-restrict");
    maker.product_gtin("Other", "Acme", perch_tests::world::GTIN_B);
    let others: Vec<String> = (0..3)
        .map(|i| seller.listing(&perch_tests::world::key_b(), &format!("https://shop.example/b{i}"), 500).id)
        .collect();
    let mut crowd = w.crowd("sp-restrict-f", 9);
    for (i, c) in crowd.iter_mut().enumerate() {
        c.flag(&others[i % 3], "fraud");
    }
    let mut logs = vec![&maker, &seller];
    logs.extend(crowd.iter());
    let s = load(&w, &logs);
    let pv = product_view(&s, &w.policy(), &key_a());
    assert_eq!(pv["listings"][0]["seller_restricted"], json!(true), "control: seller is restricted");
    check(&w, &s, "restricted seller");
}

#[test]
fn sparse_view_matches_with_revoked_and_alt_key_cases() {
    require_sut!(Views);
    let (mut w, maker, seller, l) = base("sp-mix");
    // Seller's alternate key flags a negative review; a revoked key reviews.
    let mut buyer = w.verified("sp-mix-buyer", "sp-mix-buyer");
    let rev = buyer.review(&key_a(), 1, None);
    let mut alt = w.verified("sp-mix-alt", "sp-mix-seller");
    alt.flag(&rev.id, "misleading");
    let mut stolen = w.verified("sp-mix-stolen", "sp-mix-stolen");
    stolen.profile("S", "buyer");
    stolen.review(&key_a(), 5, None);
    w.ida.attest_revoke(&stolen.author(), 1);
    let mut crowd = w.crowd("sp-mix-f", 2);
    for c in crowd.iter_mut() {
        c.flag(&l, "fraud");
    }
    let mut logs = vec![&maker, &seller, &buyer, &alt, &stolen];
    logs.extend(crowd.iter());
    let s = load(&w, &logs);
    check(&w, &s, "revoked and alt-key cases");
}
