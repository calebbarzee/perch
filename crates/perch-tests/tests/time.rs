//! Time rules (contract 7.2).

use perch_tests::fixture::{Log, Opts};
use perch_tests::require_sut;
use perch_tests::sut::{Store, product_view};
use perch_tests::world::{NOW, World, key_a, product_and_listing};
use serde_json::json;

#[test]
fn future_ts_ignored() {
    require_sut!();
    let mut w = World::new();
    let mut maker = Log::new("time-maker");
    let mut seller = w.seller("time-seller", "shop.example");
    product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let mut r = w.verified("time-reviewer", "time-r");
    r.add_with(
        "review",
        json!({ "product": key_a(), "rating": 1, "text": "From the future." }),
        Opts { ts: Some(NOW + 600_001), ..Default::default() },
    );
    let mut s = Store::new();
    for l in w.lines(&[&maker, &seller, &r]) {
        s.ingest(&l);
    }
    let p = w.policy();
    assert_eq!(product_view(&s, &p, &key_a())["ratings"]["count"], json!(0));
    let mut later = p.clone();
    later["now"] = json!(NOW + 600_001);
    assert_eq!(product_view(&s, &later, &key_a())["ratings"]["count"], json!(1), "exists once now catches up");
}

#[test]
fn views_are_pure_functions_of_policy_now() {
    require_sut!();
    let mut w = World::new();
    let mut maker = Log::new("time2-maker");
    let mut seller = w.seller("time2-seller", "shop.example");
    product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let mut s = Store::new();
    for l in w.lines(&[&maker, &seller]) {
        s.ingest(&l);
    }
    let p = w.policy();
    let a = product_view(&s, &p, &key_a());
    std::thread::sleep(std::time::Duration::from_millis(5));
    let b = product_view(&s, &p, &key_a());
    assert_eq!(a, b);
}
