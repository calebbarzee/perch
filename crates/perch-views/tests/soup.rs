//! Property test: a random soup of signed records never makes a view panic.
//!
//! Records have random kinds (including unknown ones), random `replaces`,
//! targets, products, and receipts; some are header-only, some fork their
//! author's log, some are dated in the future. Every view function runs over
//! a full store and a sparse store, under two policies.

mod common;

use common::*;
use perch_core::{ProductKey, Record, RecordId};
use perch_demo::Author;
use perch_store::{MemoryStore, Store};
use perch_views::*;
use proptest::prelude::*;
use serde_json::{Value, json};

const KINDS: [&str; 15] = [
    "profile", "attest.identity", "attest.domain", "attest.revoke", "product", "listing", "observation", "review",
    "flag", "appeal", "retract", "retire", "redact", "x.unknown", "review",
];

#[derive(Debug, Clone)]
struct Op {
    author: u8,
    kind: u8,
    variant: u8,
    replaces: Option<u16>,
    target: u16,
    product: u8,
    strip: bool,
    fork: bool,
    future: bool,
}

fn op() -> impl Strategy<Value = Op> {
    (any::<u8>(), any::<u8>(), any::<u8>(), prop::option::weighted(0.25, any::<u16>()), any::<u16>(), any::<u8>(), prop::bool::weighted(0.2), prop::bool::weighted(0.04), prop::bool::weighted(0.03))
        .prop_map(|(author, kind, variant, replaces, target, product, strip, fork, future)| Op {
            author, kind, variant, replaces, target, product, strip, fork, future,
        })
}

struct Soup {
    authors: Vec<Author>,
    records: Vec<Record>,
    keys: Vec<ProductKey>,
}

/// Picks a record: usually one of the last 8, so records interact.
fn pick<T: Clone>(v: &[T], i: u16) -> Option<T> {
    if v.is_empty() {
        return None;
    }
    let n = v.len();
    let idx = if i % 4 != 0 { n - 1 - (usize::from(i / 4) % n.min(8)) } else { usize::from(i) % n };
    Some(v[idx].clone())
}

fn body(s: &Soup, o: &Op, kind: &str) -> Value {
    let a = |i: u16| s.authors[i as usize % s.authors.len()].id().to_hex();
    let rid = |i: u16| pick(&s.records, i).map(|r| r.id()).unwrap_or(RecordId([7; 32])).to_hex();
    let key = s.keys[o.product as usize % s.keys.len()].as_str().to_owned();
    let v = o.variant;
    let mut b = match kind {
        "profile" => json!({ "name": format!("N{v}"), "role": (["buyer", "seller", "attestor"][v as usize % 3]) }),
        "attest.identity" => json!({ "subject": a(o.target), "person": hex_person(&format!("p{}", v % 4)),
                                     "level": "person", "expires": if v % 5 == 0 { BASE } else { EXP } }),
        "attest.domain" => json!({ "subject": a(o.target), "domain": format!("d{}.example", v % 3), "expires": EXP }),
        "attest.revoke" => json!({ "subject": a(o.target), "valid_through_seq": v % 6 }),
        "product" => {
            let mut p = json!({ "name": format!("Item {}", v % 7), "brand": format!("B{}", v % 3) });
            match v % 4 {
                0 => p["gtin"] = json!((["4006381333931", "4006381333948", "4006381333955"][o.product as usize % 3])),
                1 => p["mpn"] = json!(format!("M-{}", v % 5)),
                2 => p["category"] = json!("home/misc"),
                _ => {}
            }
            p
        }
        "listing" => json!({ "product": key, "url": format!("https://www.d{}.example/x", v % 3),
                             "price": { "amount": u64::from(v) * 10, "currency": if v % 2 == 0 { "EUR" } else { "USD" } },
                             "availability": (["in_stock", "out_of_stock", "preorder"][v as usize % 3]), "condition": "new" }),
        "observation" => json!({ "product": key, "url": "https://obs.example/x", "price": { "amount": v, "currency": "EUR" },
                                 "observed_at": BASE, "listing": rid(o.target) }),
        "review" => {
            let mut r = json!({ "product": key, "rating": 1 + v % 5, "text": "t" });
            if v % 3 == 0
                && let Some(l) = pick(&s.records, o.target)
            {
                let seller = &s.authors[o.target as usize % s.authors.len()];
                let buyer = &s.authors[o.author as usize % s.authors.len()];
                r["receipt"] = receipt(seller, l.id(), buyer).to_value();
                r["listing"] = json!(l.id().to_hex());
            }
            r
        }
        "flag" => json!({ "target": rid(o.target), "reason": "fraud" }),
        "appeal" => json!({ "target": rid(o.target), "note": "n" }),
        "retract" => json!({ "target": rid(o.target) }),
        "retire" => json!({}),
        "redact" => {
            let mut r = json!({ "target": rid(o.target), "reason": "other" });
            if v % 2 == 0 {
                r["product"] = json!(key);
            }
            r
        }
        _ => json!({ "anything": v }),
    };
    // Some bodies break the schema on purpose.
    if v % 11 == 0 {
        b["rating"] = json!(9);
        b["url"] = json!("http://bad");
    }
    b
}

fn build(ops: &[Op]) -> Soup {
    let attestor = Author::from_label("test/attestor");
    let mut authors = vec![attestor];
    for i in 0..6 {
        authors.push(Author::from_label(&format!("soup/{i}")));
    }
    let keys = vec![
        ProductKey::from_gtin("4006381333931").unwrap(),
        ProductKey::from_gtin("4006381333948").unwrap(),
        ProductKey::from_brand_mpn("B1", "M-1").unwrap(),
    ];
    let mut s = Soup { authors, records: Vec::new(), keys };
    for (i, o) in ops.iter().enumerate() {
        let kind = KINDS[o.kind as usize % KINDS.len()];
        let b = body(&s, o, kind);
        let ai = o.author as usize % s.authors.len();
        // Half of the replacements name the author's latest record of the same kind.
        let replaces = o.replaces.and_then(|r| {
            if r % 2 == 0 {
                let me = s.authors[ai].id();
                s.records.iter().rev().find(|x| x.author() == me && x.kind() == kind).map(|x| x.id())
            } else {
                pick(&s.records, r).map(|x| x.id())
            }
        });
        let ts = if o.future { NOW + 365 * DAY } else { BASE + i as u64 * 60_000 };
        let rec = if o.fork && s.authors[ai].next_seq() > 1 {
            s.authors[ai].publish_conflicting(ts, kind, b, replaces)
        } else {
            s.authors[ai].publish(ts, kind, b, replaces)
        };
        // A product record with a rec: key joins the key pool.
        if kind == "product"
            && let Some(m) = rec.body()
            && let Ok(k) = perch_core::kinds::derive_product_key(m, rec.id())
        {
            s.keys.push(k);
        }
        s.records.push(if o.strip { rec.without_body() } else { rec });
    }
    s
}

fn exercise<S: Store>(store: &S, policy: &Policy, soup: &Soup) {
    let v = Views::new(store, policy);
    let mut keys: Vec<String> = store.product_keys();
    keys.extend(soup.keys.iter().map(|k| k.as_str().to_owned()));
    for k in &keys {
        let _ = canonical_json(&v.product(k));
        let _ = related_records(store, k, policy);
    }
    for r in &soup.records {
        let id = r.id();
        let _ = canonical_json(&v.listing(&id));
        let _ = v.flags(&id);
        let _ = v.record_status(&id);
        let _ = v.chain_status(&id);
    }
    for a in &soup.authors {
        let _ = canonical_json(&v.author(&a.id()));
        let _ = v.author_records(&a.id(), &[], 50, 1);
        let _ = admission_tier(store, policy, &a.id());
        let _ = person_keys(store, policy, &a.id());
    }
    for q in ["", "item", "b1", "4006381333931", "m"] {
        for sort in [Sort::Relevance, Sort::Price, Sort::RatingCount, Sort::RatingMean] {
            let _ = canonical_json(&v.search(q, &SearchOptions { sort, limit: 3, ..SearchOptions::default() }));
        }
    }
    let _ = v.stats();
    let _ = v.invalid_records();
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 200, ..ProptestConfig::default() })]

    #[test]
    fn random_record_soup_never_panics(ops in prop::collection::vec(op(), 1..90)) {
        let soup = build(&ops);
        let mut full = MemoryStore::new();
        let mut sparse = MemoryStore::new_sparse();
        for r in &soup.records {
            full.insert(r.clone());
            sparse.insert(r.clone());
        }
        let mut p = default_policy(NOW, &[soup.authors[0].id(), soup.authors[1].id()]);
        p.flag_threshold = 1;
        exercise(&full, &p, &soup);
        exercise(&sparse, &p, &soup);
        p.flags_require_verified = false;
        p.verified_purchase_requires_verified_buyer = false;
        p.price_requires_domain_verified = false;
        p.trusted_attestors[0].not_after_seq = Some(3);
        exercise(&full, &p, &soup);
    }

    #[test]
    fn random_soup_related_set_matches_full_store(ops in prop::collection::vec(op(), 1..70)) {
        let soup = build(&ops);
        let mut full = MemoryStore::new();
        for r in &soup.records {
            full.insert(r.clone());
        }
        let p = default_policy(NOW, &[soup.authors[0].id(), soup.authors[1].id()]);
        for k in full.product_keys() {
            let mut sparse = MemoryStore::new_sparse();
            for id in related_records(&full, &k, &p) {
                if let Some(r) = full.get(&id) {
                    sparse.insert(r);
                }
            }
            prop_assert_eq!(canonical_json(&product_view(&full, &p, &k)), canonical_json(&product_view(&sparse, &p, &k)), "key {}", k);
        }
    }
}
