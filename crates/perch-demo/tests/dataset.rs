//! Determinism and validity of the generated datasets.

use std::collections::BTreeMap;

use perch_core::kinds::check_record;
use perch_core::{AuthorId, Record, sha256};
use perch_demo::{DEMO_NOW, DEMO_SEED, Size, generate, generate_demo};

fn jsonl_hash(records: &[Record]) -> String {
    perch_core::ids::encode_hex(&sha256(perch_core::to_jsonl(records.iter()).as_bytes()))
}

#[test]
fn repeated_runs_are_byte_identical() {
    let a = generate_demo(DEMO_SEED, Size::Small);
    let b = generate_demo(DEMO_SEED, Size::Small);
    assert_eq!(a.to_jsonl(), b.to_jsonl());
    assert_eq!(a.manifest(), b.manifest());
    let c = generate(DEMO_SEED + 1, Size::Small);
    assert_ne!(jsonl_hash(&a.records), jsonl_hash(&c), "different seeds give different data");
}

#[test]
fn small_dataset_hash() {
    // Prints the hash so cross-platform runs (wasm) can compare it.
    let d = generate_demo(DEMO_SEED, Size::Small);
    println!("small dataset: {} records, sha256 {}", d.records.len(), jsonl_hash(&d.records));
}

fn check_sizes(size: Size) {
    let d = generate_demo(DEMO_SEED, size);
    let n = d.records.len();
    let t = size.target();
    assert!(n * 100 >= t * 85 && n * 100 <= t * 115, "{size:?}: {n} records, target {t}");
}

#[test]
fn small_size_near_target() {
    check_sizes(Size::Small);
}

#[test]
fn medium_size_near_target() {
    check_sizes(Size::Medium);
}

#[test]
#[ignore = "slow in debug builds; run with --release"]
fn large_size_near_target() {
    check_sizes(Size::Large);
}

/// Every record round-trips through strict parsing; logs are complete,
/// linked, and have non-decreasing ts, except the one forked author.
fn check_logs(size: Size) {
    let d = generate_demo(DEMO_SEED, size);
    let forked = d.actor("mallory");
    let mut logs: BTreeMap<AuthorId, Vec<&Record>> = BTreeMap::new();
    for r in &d.records {
        let back = Record::from_json_str(&r.to_json_string()).expect("isolated-valid");
        assert_eq!(back.id(), r.id());
        logs.entry(r.author()).or_default().push(r);
    }
    for (author, log) in logs {
        // The forked author's twin record repeats a seq; skipping repeats keeps one branch.
        let mut prev = None;
        let mut last_ts = 0;
        let mut expect = 1;
        for r in log {
            if author == forked && r.seq() != expect {
                continue;
            }
            assert_eq!(r.seq(), expect, "author {author} seq");
            if r.seq() > 1 {
                assert_eq!(r.prev(), prev, "author {author} prev at seq {}", r.seq());
            }
            assert!(r.ts() >= last_ts, "author {author} ts decreases at seq {}", r.seq());
            prev = Some(r.id());
            last_ts = r.ts();
            expect += 1;
        }
    }
}

#[test]
fn logs_are_valid_small() {
    check_logs(Size::Small);
}

#[test]
fn logs_are_valid_medium() {
    check_logs(Size::Medium);
}

#[test]
fn fork_evidence_is_a_fork() {
    let d = generate_demo(DEMO_SEED, Size::Small);
    let [a, b] = &d.fork_evidence;
    let pair = perch_core::EvidencePair::new(a.clone(), b.clone()).expect("evidence");
    assert_eq!(pair.author(), d.actor("mallory"));
    assert!(d.records.iter().any(|r| r.id() == a.id()));
    assert!(d.records.iter().any(|r| r.id() == b.id()));
}

#[test]
fn schema_checks_match_scenario_intent() {
    let d = generate_demo(DEMO_SEED, Size::Medium);
    let schema_invalid = ["invalid_listing_http", "invalid_review_rating", "invalid_product_gtin"];
    let ids: BTreeMap<_, _> = d.records_by_name.iter().map(|(k, v)| (*v, k.as_str())).collect();
    let mut seen = 0;
    for r in &d.records {
        let name = ids.get(&r.id()).copied().unwrap_or("");
        let res = check_record(r);
        if schema_invalid.contains(&name) {
            assert!(res.is_err(), "{name} should fail the schema");
            seen += 1;
        } else if !r.has_body() {
            continue;
        } else {
            assert!(res.is_ok(), "record {} ({name}, kind {}) fails schema: {:?}", r.id(), r.kind(), res);
        }
    }
    assert_eq!(seen, schema_invalid.len());
    let unknown = d.records.iter().filter(|r| !perch_core::kinds::is_known_kind(r.kind())).count();
    assert_eq!(unknown, 2);
    let header_only = d.records.iter().filter(|r| !r.has_body()).count();
    assert_eq!(header_only, 2);
}

#[test]
fn only_the_future_listing_is_after_now() {
    let d = generate_demo(DEMO_SEED, Size::Medium);
    let future: Vec<_> = d.records.iter().filter(|r| r.ts() > DEMO_NOW).map(|r| r.id()).collect();
    assert_eq!(future, vec![d.record("kettle_future")]);
}

#[test]
fn named_parts_exist() {
    let d = generate_demo(DEMO_SEED, Size::Small);
    for name in ["kettle", "headphones", "tent", "lamp", "charger", "cable", "mug", "puzzle"] {
        let k = d.product(name);
        assert!(!k.as_str().is_empty());
    }
    assert!(d.product("lamp").as_str().starts_with("mpn:"));
    assert!(d.product("mug").as_str().starts_with("rec:"));
    assert!(d.product("kettle").as_str().starts_with("gtin:"));
    assert_eq!(d.attestors.len(), 2);
    assert!(d.attestor_b_honest_through_seq > 0);
    let m = d.manifest();
    assert!(m["actors"]["seller_good"].is_string());
}
