//! Semantic reference rules (contract 4.5, 4.6): replaces, retract, fixed fields.

use perch_tests::fixture::{Log, Opts};
use perch_tests::require_sut;
use perch_tests::sut::{Store, listing_view, product_view};
use perch_tests::world::{World, key_a, key_b, product_and_listing};
use serde_json::{Value, json};

fn store_of(w: &World, logs: &[&Log]) -> Store {
    let mut s = Store::new();
    for l in w.lines(logs) {
        s.ingest(&l);
    }
    s
}

fn listing_body(product: &str, amount: i64) -> Value {
    json!({
        "product": product,
        "url": "https://shop.example/widget",
        "price": { "amount": amount, "currency": "EUR" },
        "availability": "in_stock",
        "condition": "new"
    })
}

#[test]
fn replacement_updates_price_and_keeps_root() {
    require_sut!();
    let mut w = World::new();
    let mut maker = Log::new("ref-maker");
    let mut seller = w.seller("ref-seller", "shop.example");
    let (_p, l1) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let l2 = seller.add_with("listing", listing_body(&key_a(), 900), Opts { replaces: Some(l1.id.clone()), ..Default::default() });
    let s = store_of(&w, &[&maker, &seller]);
    let v = listing_view(&s, &w.policy(), &l1.id);
    assert_eq!(v["id"], json!(l1.id));
    assert_eq!(v["latest"], json!(l2.id));
    assert_eq!(v["price"]["amount"], json!(900));
}

#[test]
fn replaces_other_author_ignored() {
    require_sut!();
    let mut w = World::new();
    let mut maker = Log::new("ref-maker2");
    let mut seller = w.seller("ref-victim", "shop.example");
    let mut mallory = w.seller("ref-mallory", "evil.example");
    let (_p, l1) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    // Mallory claims to replace the victim's listing with a lower price.
    mallory.add_with("listing", listing_body(&key_a(), 1), Opts { replaces: Some(l1.id.clone()), ..Default::default() });
    let s = store_of(&w, &[&maker, &seller, &mallory]);
    let v = listing_view(&s, &w.policy(), &l1.id);
    assert_eq!(v["latest"], json!(l1.id), "cross-author replaces must be ignored");
    assert_eq!(v["price"]["amount"], json!(1000));
}

#[test]
fn replaces_other_kind_ignored() {
    require_sut!();
    let mut w = World::new();
    let mut maker = Log::new("ref-maker3");
    let mut seller = w.seller("ref-kind", "shop.example");
    let (_p, l1) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    // A profile record that claims to replace the listing.
    seller.add_with("profile", json!({ "name": "x", "role": "seller" }), Opts { replaces: Some(l1.id.clone()), ..Default::default() });
    let s = store_of(&w, &[&maker, &seller]);
    let v = listing_view(&s, &w.policy(), &l1.id);
    assert_eq!(v["latest"], json!(l1.id));
    assert_eq!(v["status"], json!("visible"));
}

#[test]
fn replacement_changing_fixed_field_ignored() {
    require_sut!();
    // listing.product is fixed: moving a listing (and its reviews) to product B is invalid.
    let mut w = World::new();
    let mut maker = Log::new("ref-maker4");
    let mut seller = w.seller("ref-fixed", "shop.example");
    let (_p, l1) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    maker.product_gtin("Other", "Acme", perch_tests::world::GTIN_B);
    seller.add_with("listing", listing_body(&key_b(), 500), Opts { replaces: Some(l1.id.clone()), ..Default::default() });
    let s = store_of(&w, &[&maker, &seller]);
    let v = listing_view(&s, &w.policy(), &l1.id);
    assert_eq!(v["product"], json!(key_a()));
    assert_eq!(v["price"]["amount"], json!(1000));
    let pb = product_view(&s, &w.policy(), &key_b());
    let moved = pb["listings"].as_array().map(|a| a.iter().any(|x| x["id"] == json!(l1.id))).unwrap_or(false);
    assert!(!moved, "listing must not appear under product B");
}

#[test]
fn replaces_unknown_record_ignored() {
    require_sut!();
    // `replaces` must name an earlier record in the same log; an unknown ID is invalid.
    let mut w = World::new();
    let mut seller = w.seller("ref-unknown", "shop.example");
    seller.add_with(
        "profile",
        json!({ "name": "bogus", "role": "seller" }),
        Opts { replaces: Some("0".repeat(64)), ..Default::default() },
    );
    let s = store_of(&w, &[&seller]);
    let av = perch_tests::sut::author_view(&s, &w.policy(), &seller.author());
    assert_eq!(av["name"], json!("ref-unknown"), "the original profile stays the latest valid profile");
}

#[test]
fn retract_of_other_authors_record_ignored() {
    require_sut!();
    let mut w = World::new();
    let mut maker = Log::new("ref-maker5");
    let mut seller = w.seller("ref-retract-victim", "shop.example");
    let mut mallory = w.verified("ref-retract-mallory", "mallory");
    let (_p, l1) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    mallory.retract(&l1.id);
    let s = store_of(&w, &[&maker, &seller, &mallory]);
    let v = listing_view(&s, &w.policy(), &l1.id);
    assert_eq!(v["status"], json!("visible"));
}

#[test]
fn own_retract_retracts_whole_chain() {
    require_sut!();
    let mut w = World::new();
    let mut maker = Log::new("ref-maker6");
    let mut seller = w.seller("ref-own-retract", "shop.example");
    let (_p, l1) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let l2 = seller.add_with("listing", listing_body(&key_a(), 900), Opts { replaces: Some(l1.id.clone()), ..Default::default() });
    // Retract by naming the latest version: applies to the root chain.
    seller.retract(&l2.id);
    let s = store_of(&w, &[&maker, &seller]);
    assert_eq!(listing_view(&s, &w.policy(), &l1.id)["status"], json!("retracted"));
}

#[test]
fn retraction_is_final() {
    require_sut!();
    let mut w = World::new();
    let mut maker = Log::new("ref-maker7");
    let mut seller = w.seller("ref-final", "shop.example");
    let (_p, l1) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    seller.retract(&l1.id);
    seller.add_with("listing", listing_body(&key_a(), 1), Opts { replaces: Some(l1.id.clone()), ..Default::default() });
    let s = store_of(&w, &[&maker, &seller]);
    let v = listing_view(&s, &w.policy(), &l1.id);
    assert_eq!(v["status"], json!("retracted"));
    assert_eq!(v["price"]["amount"], json!(1000), "post-retract replacement ignored");
}

#[test]
fn retract_of_control_kind_ignored() {
    require_sut!();
    // `retract` cannot target retract, retire, attest.revoke, redact (4.5).
    let mut w = World::new();
    let mut maker = Log::new("ref-maker8");
    let mut seller = w.seller("ref-ctl", "shop.example");
    let (_p, l1) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let r = seller.retract(&l1.id);
    seller.retract(&r.id); // un-retract attempt
    let s = store_of(&w, &[&maker, &seller]);
    assert_eq!(listing_view(&s, &w.policy(), &l1.id)["status"], json!("retracted"));
}

#[test]
fn flags_survive_price_update() {
    require_sut!();
    let mut w = World::new();
    let mut maker = Log::new("ref-maker9");
    let mut seller = w.seller("ref-shed", "shop.example");
    let (_p, l1) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let mut crowd = w.crowd("ref-flagger", 3);
    for c in crowd.iter_mut() {
        c.flag(&l1.id, "fraud");
    }
    // Seller publishes a new version to shed the flags.
    seller.add_with("listing", listing_body(&key_a(), 999), Opts { replaces: Some(l1.id.clone()), ..Default::default() });
    let mut logs: Vec<&Log> = vec![&maker, &seller];
    logs.extend(crowd.iter());
    let s = store_of(&w, &logs);
    let v = listing_view(&s, &w.policy(), &l1.id);
    assert_eq!(v["status"], json!("hidden"), "flags apply to the chain root");
    assert_eq!(v["flags"]["counted"], json!(3));
}

#[test]
fn flag_on_later_version_counts_for_root() {
    require_sut!();
    let mut w = World::new();
    let mut maker = Log::new("ref-maker10");
    let mut seller = w.seller("ref-ver", "shop.example");
    let (_p, l1) = product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let l2 = seller.add_with("listing", listing_body(&key_a(), 999), Opts { replaces: Some(l1.id.clone()), ..Default::default() });
    let mut crowd = w.crowd("ref-vflag", 3);
    crowd[0].flag(&l1.id, "fraud");
    crowd[1].flag(&l2.id, "fraud");
    crowd[2].flag(&l2.id, "fraud");
    let mut logs: Vec<&Log> = vec![&maker, &seller];
    logs.extend(crowd.iter());
    let s = store_of(&w, &logs);
    assert_eq!(listing_view(&s, &w.policy(), &l1.id)["flags"]["counted"], json!(3));
}
