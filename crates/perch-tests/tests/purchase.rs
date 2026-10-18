//! Verified purchase (contract 4.7, 7.5).

use perch_tests::fixture::{Key, Log, receipt};
use perch_tests::require_sut;
use perch_tests::sut::{Store, product_view, verify_receipt};
use perch_tests::world::{EXPIRES, World, key_a, product_and_listing};
use serde_json::{Value, json};

fn store_of(w: &World, logs: &[&Log]) -> Store {
    let mut s = Store::new();
    for l in w.lines(logs) {
        s.ingest(&l);
    }
    s
}

/// The review view for `review_root` in product A.
fn review(s: &Store, p: &Value, review_root: &str) -> Value {
    let v = product_view(s, p, &key_a());
    v["reviews"]
        .as_array()
        .and_then(|a| a.iter().find(|r| r["id"] == json!(review_root)).cloned())
        .unwrap_or(Value::Null)
}

struct Shop {
    w: World,
    maker: Log,
    seller: Log,
    listing: String,
    buyer: Log,
}

fn shop(tag: &str) -> Shop {
    let mut w = World::new();
    let mut maker = Log::new(&format!("{tag}-maker"));
    let mut seller = w.seller(&format!("{tag}-seller"), "shop.example");
    let (_p, l) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let mut buyer = w.verified(&format!("{tag}-buyer"), &format!("{tag}-buyer"));
    buyer.profile("Buyer", "buyer");
    Shop { w, maker, seller, listing: l.id, buyer }
}

#[test]
fn valid_receipt_is_verified_purchase() {
    require_sut!();
    let mut s = shop("vp-ok");
    let rc = s.seller.receipt(&s.listing, &s.buyer.author(), perch_tests::fixture::T0, "n1");
    assert_eq!(verify_receipt(&rc, &s.seller.author()), Ok(()));
    let r = s.buyer.review(&key_a(), 2, Some(rc));
    let st = store_of(&s.w, &[&s.maker, &s.seller, &s.buyer]);
    assert_eq!(review(&st, &s.w.policy(), &r.id)["verified_purchase"], json!(true));
}

#[test]
fn receipt_by_non_seller_not_verified() {
    require_sut!();
    let mut s = shop("vp-nonseller");
    let other = Key::from_label("vp-nonseller-other");
    let rc = receipt(&other, &s.listing, &s.buyer.author(), 1, "n");
    let r = s.buyer.review(&key_a(), 5, Some(rc));
    let st = store_of(&s.w, &[&s.maker, &s.seller, &s.buyer]);
    assert_eq!(review(&st, &s.w.policy(), &r.id)["verified_purchase"], json!(false));
}

#[test]
fn review_citing_someone_elses_receipt_not_verified() {
    require_sut!();
    let mut s = shop("vp-steal");
    let rc = s.seller.receipt(&s.listing, &s.buyer.author(), 1, "n");
    let mut thief = s.w.verified("vp-steal-thief", "thief");
    let r = thief.review(&key_a(), 1, Some(rc));
    let st = store_of(&s.w, &[&s.maker, &s.seller, &s.buyer, &thief]);
    assert_eq!(review(&st, &s.w.policy(), &r.id)["verified_purchase"], json!(false));
}

#[test]
fn receipt_for_other_product_not_verified() {
    require_sut!();
    let mut s = shop("vp-otherprod");
    s.maker.product_gtin("Other", "Acme", perch_tests::world::GTIN_B);
    let lb = s.seller.listing(&perch_tests::world::key_b(), "https://shop.example/other", 5);
    let rc = s.seller.receipt(&lb.id, &s.buyer.author(), 1, "n");
    let r = s.buyer.review(&key_a(), 5, Some(rc));
    let st = store_of(&s.w, &[&s.maker, &s.seller, &s.buyer]);
    assert_eq!(review(&st, &s.w.policy(), &r.id)["verified_purchase"], json!(false));
}

#[test]
fn seller_review_with_own_receipt_not_verified() {
    require_sut!();
    let mut s = shop("vp-selfrev");
    let me = s.seller.author();
    let rc = s.seller.receipt(&s.listing, &me, 1, "n");
    let r = s.seller.review(&key_a(), 5, Some(rc));
    let st = store_of(&s.w, &[&s.maker, &s.seller, &s.buyer]);
    let v = review(&st, &s.w.policy(), &r.id);
    assert_eq!(v["verified_purchase"], json!(false));
    let ratings = &product_view(&st, &s.w.policy(), &key_a())["ratings"];
    assert_eq!(ratings["count"], json!(0), "reviews by a product's sellers do not count in aggregates");
}

#[test]
fn unverified_seller_receipt_not_verified() {
    require_sut!();
    let mut w = World::new();
    let mut maker = Log::new("vp-unv-maker");
    let mut seller = Log::new("vp-unv-seller"); // no attestation
    let (_p, l) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let mut buyer = w.verified("vp-unv-buyer", "unv-buyer");
    let rc = seller.receipt(&l.id, &buyer.author(), 1, "n");
    let r = buyer.review(&key_a(), 5, Some(rc));
    let st = store_of(&w, &[&maker, &seller, &buyer]);
    assert_eq!(review(&st, &w.policy(), &r.id)["verified_purchase"], json!(false));
}

#[test]
fn sybil_buyers_of_verified_seller_not_verified_purchase() {
    require_sut!();
    // Seller issues receipts to 20 fresh unverified keys (threat T25).
    let s = shop("vp-sybil");
    let mut sybils: Vec<Log> = (0..20).map(|i| Log::new(&format!("vp-sybil-{i}"))).collect();
    let mut ids = vec![];
    for (i, b) in sybils.iter_mut().enumerate() {
        let rc = s.seller.receipt(&s.listing, &b.author(), 1, &format!("n{i}"));
        ids.push(b.review(&key_a(), 5, Some(rc)).id);
    }
    let mut logs: Vec<&Log> = vec![&s.maker, &s.seller, &s.buyer];
    logs.extend(sybils.iter());
    let st = store_of(&s.w, &logs);
    let p = s.w.policy();
    for id in &ids {
        assert_eq!(review(&st, &p, id)["verified_purchase"], json!(false));
    }
    let r = &product_view(&st, &p, &key_a())["ratings"];
    assert_eq!(r["verified_purchase"]["count"], json!(0));
    assert_eq!(r["verified_identity"]["count"], json!(0));
}

#[test]
fn tampered_receipt_not_verified() {
    require_sut!();
    let mut s = shop("vp-tamper");
    let mut rc = s.seller.receipt(&s.listing, &s.buyer.author(), 1, "n");
    rc["purchased_at"] = json!(2);
    assert!(verify_receipt(&rc, &s.seller.author()).is_err());
    let r = s.buyer.review(&key_a(), 5, Some(rc));
    let st = store_of(&s.w, &[&s.maker, &s.seller, &s.buyer]);
    assert_eq!(review(&st, &s.w.policy(), &r.id)["verified_purchase"], json!(false));
}

#[test]
fn receipt_survives_listing_retract() {
    require_sut!();
    let mut s = shop("vp-retract");
    let rc = s.seller.receipt(&s.listing, &s.buyer.author(), 1, "n");
    let r = s.buyer.review(&key_a(), 1, Some(rc));
    let lid = s.listing.clone();
    s.seller.retract(&lid);
    let st = store_of(&s.w, &[&s.maker, &s.seller, &s.buyer]);
    assert_eq!(review(&st, &s.w.policy(), &r.id)["verified_purchase"], json!(true));
}

#[test]
fn receipt_after_seller_revoke_cutoff_not_verified() {
    require_sut!();
    // A listing created after the seller's revoke cutoff cannot anchor receipts (4.7).
    let mut s = shop("vp-revoked");
    let cutoff = s.seller.head().unwrap().seq();
    let l2 = s.seller.listing(&key_a(), "https://shop.example/w2", 1);
    s.w.ida.attest_revoke(&s.seller.author(), cutoff);
    let rc = s.seller.receipt(&l2.id, &s.buyer.author(), 1, "n");
    let r = s.buyer.review(&key_a(), 5, Some(rc));
    let st = store_of(&s.w, &[&s.maker, &s.seller, &s.buyer]);
    assert_eq!(review(&st, &s.w.policy(), &r.id)["verified_purchase"], json!(false));
}

#[test]
fn one_counted_review_per_author_and_product() {
    require_sut!();
    let mut s = shop("vp-dup");
    for i in 0..3 {
        let rc = s.seller.receipt(&s.listing, &s.buyer.author(), i, &format!("n{i}"));
        s.buyer.review(&key_a(), 5, Some(rc));
    }
    let st = store_of(&s.w, &[&s.maker, &s.seller, &s.buyer]);
    let r = &product_view(&st, &s.w.policy(), &key_a())["ratings"];
    assert_eq!(r["verified_purchase"]["count"], json!(1));
    assert_eq!(r["count"], json!(1));
}

#[test]
fn unverified_buyer_not_verified_purchase_by_default() {
    require_sut!();
    let s = shop("vp-unvbuyer");
    let mut anon = Log::new("vp-unvbuyer-anon");
    let rc = s.seller.receipt(&s.listing, &anon.author(), 1, "n");
    let r = anon.review(&key_a(), 4, Some(rc));
    let st = store_of(&s.w, &[&s.maker, &s.seller, &s.buyer, &anon]);
    let mut p = s.w.policy();
    assert_eq!(review(&st, &p, &r.id)["verified_purchase"], json!(false));
    p["verified_purchase_requires_verified_buyer"] = json!(false);
    assert_eq!(review(&st, &p, &r.id)["verified_purchase"], json!(true));
    let _ = EXPIRES;
}
