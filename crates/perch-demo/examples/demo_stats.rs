//! Prints record counts per kind and generation time for each size.
//! `cargo run --release -p perch-demo --example demo_stats [small|medium|large]`

use std::collections::BTreeMap;

fn main() {
    let sizes: Vec<perch_demo::Size> = match std::env::args().nth(1) {
        Some(s) => vec![perch_demo::Size::parse(&s).expect("small, medium, or large")],
        None => vec![perch_demo::Size::Small, perch_demo::Size::Medium, perch_demo::Size::Large],
    };
    for size in sizes {
        let t = std::time::Instant::now();
        let d = perch_demo::generate_demo(perch_demo::DEMO_SEED, size);
        let el = t.elapsed();
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        for r in &d.records {
            *kinds.entry(r.kind().to_owned()).or_default() += 1;
        }
        let bytes = d.to_jsonl().len();
        println!("{size:?}: {} records, {} bytes JSONL, {:.2?}", d.records.len(), bytes, el);
        for (k, n) in kinds {
            println!("  {k:16} {n}");
        }
    }
}
