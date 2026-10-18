//! Prints views of the demo dataset as canonical JSON.
//! `cargo run -p perch-views --example dump -- [small|medium|large] [product|author|listing|search|stats] [name]`

use perch_store::{MemoryStore, Store};
use perch_views::*;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let size = perch_demo::Size::parse(args.first().map(String::as_str).unwrap_or("small")).expect("size");
    let what = args.get(1).map(String::as_str).unwrap_or("stats");
    let name = args.get(2).cloned().unwrap_or_default();
    let demo = perch_demo::generate_demo(perch_demo::DEMO_SEED, size);
    let mut store = MemoryStore::new();
    for r in &demo.records {
        store.insert(r.clone());
    }
    let policy = default_policy(demo.now, &demo.attestors);
    let v = Views::new(&store, &policy);
    let out = match what {
        "product" => canonical_json(&v.product(demo.product(&name).as_str())),
        "author" => canonical_json(&v.author(&demo.actor(&name))),
        "listing" => canonical_json(&v.listing(&demo.listing(&name))),
        "search" => canonical_json(&v.search(&name, &SearchOptions::default())),
        "flags" => canonical_json(&v.flags(&demo.listing(&name))),
        "invalid" => canonical_json(&v.invalid_records()),
        _ => canonical_json(&v.stats()),
    };
    println!("{out}");
}
