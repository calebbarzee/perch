//! Timings on a demo dataset (native, one thread).
//! `cargo run --release -p perch-views --example bench -- [small|medium|large]`

use std::time::Instant;

/// CPU time of this thread in milliseconds (Linux), so other processes on a
/// shared machine do not inflate the numbers. Falls back to wall time.
#[cfg(target_os = "linux")]
fn cpu_ms() -> f64 {
    #[repr(C)]
    struct Timespec {
        sec: i64,
        nsec: i64,
    }
    unsafe extern "C" {
        fn clock_gettime(clock: i32, ts: *mut Timespec) -> i32;
    }
    let mut ts = Timespec { sec: 0, nsec: 0 };
    // CLOCK_THREAD_CPUTIME_ID = 3
    unsafe { clock_gettime(3, &mut ts) };
    ts.sec as f64 * 1000.0 + ts.nsec as f64 / 1e6
}
#[cfg(not(target_os = "linux"))]
fn cpu_ms() -> f64 {
    use std::sync::OnceLock;
    static T0: OnceLock<Instant> = OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

use perch_store::{MemoryStore, Store};
use perch_views::*;

fn main() {
    let size = perch_demo::Size::parse(&std::env::args().nth(1).unwrap_or_else(|| "large".into())).expect("size");
    let t = Instant::now();
    let demo = perch_demo::generate_demo(perch_demo::DEMO_SEED, size);
    println!("generate: {} records in {:.2?}", demo.records.len(), t.elapsed());
    let t = Instant::now();
    let mut store = MemoryStore::new();
    for r in &demo.records {
        store.insert(r.clone());
    }
    println!("insert: {:.2?}", t.elapsed());
    let policy = default_policy(demo.now, &demo.attestors);

    // Products by review count (from the index), plus scenario products.
    let mut keys: Vec<(usize, String)> = store
        .product_keys()
        .into_iter()
        .map(|k| (store.by_product(&k).len(), k))
        .collect();
    keys.sort_by(|a, b| b.cmp(a));
    let mut sample: Vec<String> = keys.iter().take(5).map(|(_, k)| k.clone()).collect();
    sample.extend(keys.iter().skip(keys.len() / 2).take(5).map(|(_, k)| k.clone()));
    sample.extend(demo.products.values().map(|k| k.as_str().to_owned()));
    let mut worst = 0f64;
    let mut total = 0f64;
    let min_ms = |f: &mut dyn FnMut()| -> f64 {
        (0..5)
            .map(|_| {
                let t = cpu_ms();
                f();
                cpu_ms() - t
            })
            .fold(f64::MAX, f64::min)
    };
    for k in &sample {
        let v = product_view(&store, &policy, k).expect("product");
        let ms = min_ms(&mut || {
            product_view(&store, &policy, k);
        });
        worst = worst.max(ms);
        total += ms;
        println!("product_view {k:40} {:4} reviews {:3} listings {ms:7.2} ms", v.reviews.len(), v.listings.len());
    }
    println!("product_view: mean {:.2} ms, max {:.2} ms CPU over {} products", total / sample.len() as f64, worst, sample.len());
    for q in ["kettle", "alder kettle", "headphones", "tent", "lumen", "2000090000018", "a", ""] {
        let hits = search(&store, &policy, q, &SearchOptions::default());
        let ms = min_ms(&mut || {
            search(&store, &policy, q, &SearchOptions::default());
        });
        println!("search {q:16} {:4} hits {ms:8.2} ms CPU (min of 5)", hits.len());
    }
    // Warm cache: one context reused across queries (node and wasm usage).
    let mut cache = ViewCache::new();
    for q in ["kettle", "", "tent"] {
        let t = cpu_ms();
        let v = Views::with_cache(&store, &policy, cache);
        let hits = v.search(q, &SearchOptions::default());
        cache = v.into_cache();
        println!("search with kept cache {q:10} {:4} hits {:8.2} ms", hits.len(), cpu_ms() - t);
    }
    let t = cpu_ms();
    let s = stats(&store);
    println!("stats: {} records, {:.2} ms", s.records, cpu_ms() - t);
    for k in sample.iter().take(3) {
        let t = Instant::now();
        let ids = related_records(&store, k, &policy);
        println!("related_records {k:40} {:5} ids {:.2?}", ids.len(), t.elapsed());
    }
}
