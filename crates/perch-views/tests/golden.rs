//! Golden snapshots of every view over the Small demo dataset.
//!
//! Files live in `tests/golden/`. Regenerate with
//! `PERCH_UPDATE_GOLDEN=1 cargo test -p perch-views --test golden`.
//! Comparison uses canonical JSON (RFC 8785), so the files may be pretty-printed.

use std::collections::BTreeMap;
use std::path::PathBuf;

use perch_demo::{DEMO_SEED, Size};
use perch_store::{MemoryStore, Store};
use perch_views::*;
use serde_json::{Value, json};

fn snapshots() -> BTreeMap<String, String> {
    let demo = perch_demo::generate_demo(DEMO_SEED, Size::Small);
    let mut store = MemoryStore::new();
    for r in &demo.records {
        store.insert(r.clone());
    }
    let policy = default_policy(demo.now, &demo.attestors);
    let v = Views::new(&store, &policy);
    let mut out = BTreeMap::new();
    let mut put = |name: &str, value: String| {
        out.insert(name.to_owned(), value);
    };

    put("policy.json", canonical_json(&json!({ "policy": policy, "hash": policy_hash(&policy) })));
    put("stats.json", canonical_json(&v.stats()));
    put("invalid.json", canonical_json(&v.invalid_records()));
    for (name, key) in &demo.products {
        put(&format!("product_{name}.json"), canonical_json(&v.product(key.as_str())));
    }
    let authors: BTreeMap<&String, Option<AuthorView>> =
        demo.actors.iter().filter(|(n, _)| !n.starts_with("brigade_")).map(|(n, a)| (n, v.author(a))).collect();
    put("authors.json", canonical_json(&authors));
    let listings: BTreeMap<&String, Option<ListingView>> = demo.listings.iter().map(|(n, id)| (n, v.listing(id))).collect();
    put("listings.json", canonical_json(&listings));
    let mut flags = BTreeMap::new();
    for name in ["headphones_hidden", "headphones_restored", "lamp_shady", "lamp_alt"] {
        flags.insert(name.to_owned(), v.flags(&demo.listing(name)));
    }
    flags.insert("headphones_hidden_review".to_owned(), v.flags(&demo.review("headphones_hidden_review")));
    put("flags.json", canonical_json(&flags));
    let statuses: BTreeMap<&String, Option<RecordStatus>> =
        demo.records_by_name.iter().map(|(n, id)| (n, v.record_status(id))).collect();
    put("record_status.json", canonical_json(&statuses));
    let mut searches = BTreeMap::new();
    for q in ["", "kettle", "alder kettle", "lx", "2000090000018", "voltix", "tent", "nomatch"] {
        for (sname, sort) in
            [("relevance", Sort::Relevance), ("price", Sort::Price), ("rating_count", Sort::RatingCount), ("rating_mean", Sort::RatingMean)]
        {
            let opts = SearchOptions { sort, ..SearchOptions::default() };
            searches.insert(format!("{q}|{sname}"), v.search(q, &opts));
        }
    }
    put("search.json", canonical_json(&searches));
    let records: BTreeMap<&String, Vec<ChainSummary>> = ["seller_good", "alice", "mallory", "bob", "carol"]
        .iter()
        .map(|n| {
            let (k, a) = demo.actors.get_key_value(*n).unwrap();
            (k, v.author_records(a, &[], 100, 1))
        })
        .collect();
    put("author_records.json", canonical_json(&records));
    let related: BTreeMap<&String, Vec<perch_core::RecordId>> =
        demo.products.iter().map(|(n, k)| (n, related_records(&store, k.as_str(), &policy))).collect();
    put("related.json", canonical_json(&related));
    let admission: BTreeMap<&String, Tier> =
        demo.actors.iter().filter(|(n, _)| !n.starts_with("brigade_")).map(|(n, a)| (n, admission_tier(&store, &policy, a))).collect();
    put("admission.json", canonical_json(&admission));
    let _ = store.len();
    out
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

#[test]
fn golden_snapshots() {
    let snaps = snapshots();
    let update = std::env::var("PERCH_UPDATE_GOLDEN").is_ok_and(|v| v == "1");
    if update {
        std::fs::create_dir_all(dir()).unwrap();
    }
    let mut failures = Vec::new();
    for (name, canonical) in &snaps {
        let path = dir().join(name);
        if update {
            let v: Value = serde_json::from_str(canonical).unwrap();
            std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap() + "\n").unwrap();
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            failures.push(format!("{name}: missing (run with PERCH_UPDATE_GOLDEN=1)"));
            continue;
        };
        let expected = canonical_json(&serde_json::from_str::<Value>(&text).unwrap());
        if &expected != canonical {
            failures.push(format!("{name}: differs from golden"));
        }
    }
    assert!(failures.is_empty(), "golden mismatches:\n{}", failures.join("\n"));
}

/// The canonical bytes of every snapshot hash to one value; printed so
/// native and wasm runs can be compared.
#[test]
fn snapshot_digest() {
    let snaps = snapshots();
    let all: String = snaps.iter().map(|(k, v)| format!("{k}\n{v}\n")).collect();
    println!("golden digest: {}", perch_core::ids::encode_hex(&perch_core::sha256(all.as_bytes())));
}
