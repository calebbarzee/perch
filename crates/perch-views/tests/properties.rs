//! Property tests: insertion order, repeated runs, and sparse related sets.

use std::collections::{BTreeMap, VecDeque};

use perch_core::{AuthorId, Record};
use perch_demo::{DEMO_SEED, Size};
use perch_store::{MemoryStore, Store};
use perch_views::*;
use proptest::prelude::*;

fn demo() -> perch_demo::Demo {
    perch_demo::generate_demo(DEMO_SEED, Size::Small)
}

/// Canonical JSON of a broad set of views.
fn digest(store: &MemoryStore, demo: &perch_demo::Demo) -> String {
    let policy = default_policy(demo.now, &demo.attestors);
    let v = Views::new(store, &policy);
    let mut out = String::new();
    for k in demo.products.values() {
        out.push_str(&canonical_json(&v.product(k.as_str())));
    }
    for a in demo.actors.values() {
        out.push_str(&canonical_json(&v.author(a)));
    }
    out.push_str(&canonical_json(&v.search("", &SearchOptions::default())));
    out.push_str(&canonical_json(&v.stats()));
    out.push_str(&canonical_json(&v.invalid_records()));
    out
}

fn store_from(records: impl IntoIterator<Item = Record>) -> MemoryStore {
    let mut s = MemoryStore::new();
    for r in records {
        s.insert(r);
    }
    s
}

/// Interleaves per-author queues using `choices` (author order preserved).
fn interleave(records: &[Record], choices: &[u32]) -> Vec<Record> {
    let mut queues: BTreeMap<AuthorId, VecDeque<Record>> = BTreeMap::new();
    for r in records {
        queues.entry(r.author()).or_default().push_back(r.clone());
    }
    let mut out = Vec::with_capacity(records.len());
    let mut i = 0;
    while !queues.is_empty() {
        let keys: Vec<AuthorId> = queues.keys().copied().collect();
        let k = keys[(choices[i % choices.len()] as usize) % keys.len()];
        i += 1;
        let q = queues.get_mut(&k).unwrap();
        out.push(q.pop_front().unwrap());
        if q.is_empty() {
            queues.remove(&k);
        }
    }
    out
}

#[test]
fn repeated_runs_are_identical() {
    let d = demo();
    let a = digest(&store_from(d.records.clone()), &d);
    let b = digest(&store_from(d.records.clone()), &d);
    assert_eq!(a, b);
    let d2 = demo();
    assert_eq!(a, digest(&store_from(d2.records.clone()), &d2));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 8, ..ProptestConfig::default() })]

    #[test]
    fn shuffled_insertion_gives_identical_views(choices in prop::collection::vec(any::<u32>(), 64..256)) {
        let d = demo();
        let baseline = digest(&store_from(d.records.clone()), &d);
        let shuffled = interleave(&d.records, &choices);
        prop_assert_eq!(baseline, digest(&store_from(shuffled), &d));
    }
}

/// Copies the related set of `key` from `full` into a sparse store.
fn sparse_for(full: &MemoryStore, key: &str, policy: &Policy) -> MemoryStore {
    let mut s = MemoryStore::new_sparse();
    for id in related_records(full, key, policy) {
        if let Some(r) = full.get(&id) {
            s.insert(r);
        }
    }
    s
}

fn check_sparse(size: Size, sample: usize) {
    let d = perch_demo::generate_demo(DEMO_SEED, size);
    let full = store_from(d.records.clone());
    let policy = default_policy(d.now, &d.attestors);
    let mut keys: Vec<String> = d.products.values().map(|k| k.as_str().to_owned()).collect();
    keys.extend(full.product_keys().into_iter().take(sample));
    let mut sizes = Vec::new();
    for key in keys {
        let sparse = sparse_for(&full, &key, &policy);
        sizes.push(sparse.len());
        let a = canonical_json(&product_view(&full, &policy, &key));
        let b = canonical_json(&product_view(&sparse, &policy, &key));
        assert_eq!(a, b, "sparse view differs for {key}");
        // A light client can name the attestors behind every badge.
        for t in &policy.trusted_attestors {
            let full_name = author_view(&full, &policy, &t.author).and_then(|v| v.name);
            let sparse_name = author_view(&sparse, &policy, &t.author).and_then(|v| v.name);
            assert!(full_name.is_some(), "demo attestor has a profile name");
            assert_eq!(full_name, sparse_name, "attestor name missing from related set of {key}");
        }
    }
    println!("{size:?}: related-set sizes {sizes:?}");
}

#[test]
fn sparse_related_set_gives_same_product_view_small() {
    check_sparse(Size::Small, 20);
}

#[test]
fn sparse_related_set_gives_same_product_view_medium() {
    check_sparse(Size::Medium, 25);
}
