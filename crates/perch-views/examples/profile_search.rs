//! Profiling driver: one cold search. Args: size query.
use perch_store::{MemoryStore, Store};
use perch_views::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let size = perch_demo::Size::parse(a.get(1).map(String::as_str).unwrap_or("medium")).unwrap();
    let q = a.get(2).cloned().unwrap_or_default();
    let demo = perch_demo::generate_demo(perch_demo::DEMO_SEED, size);
    let mut store = MemoryStore::new();
    for r in &demo.records {
        store.insert(r.clone());
    }
    let policy = default_policy(demo.now, &demo.attestors);
    let hits = search(&store, &policy, &q, &SearchOptions::default());
    println!("{} hits, {} records", hits.len(), store.len());
}
