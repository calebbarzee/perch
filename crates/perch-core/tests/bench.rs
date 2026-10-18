//! Throughput measurements. Run in release mode:
//! `cargo test -p perch-core --release --test bench -- --ignored --nocapture`

use std::time::Instant;

use perch_core::serde_json::json;
use perch_core::{Keypair, Record, Unsigned, check_record, sign_record};

#[test]
#[ignore = "benchmark; prints numbers"]
fn throughput() {
    let n: u64 = 20_000;
    let kp = Keypair::from_seed([1; 32]);
    let body = json!({
        "product": "gtin:04006381333931", "rating": 4, "title": "Solid kettle",
        "text": "Boils 1.7 l in about four minutes. The lid hinge feels cheap; otherwise fine. ".repeat(4)
    })
    .as_object()
    .unwrap()
    .clone();

    let t = Instant::now();
    let mut prev = None;
    let mut records = Vec::with_capacity(n as usize);
    for seq in 1..=n {
        let r = sign_record(&kp, Unsigned { seq, prev, ts: seq, kind: "review".into(), replaces: None, body: body.clone() }).unwrap();
        prev = Some(r.id());
        records.push(r);
    }
    let sign = n as f64 / t.elapsed().as_secs_f64();

    let lines: Vec<String> = records.iter().map(Record::to_json_string).collect();
    let bytes: usize = lines.iter().map(String::len).sum();

    let t = Instant::now();
    for l in &lines {
        std::hint::black_box(Record::from_json_str(l).unwrap());
    }
    let secs = t.elapsed().as_secs_f64();
    let verify = n as f64 / secs;

    let t = Instant::now();
    for r in &records {
        std::hint::black_box(check_record(r).unwrap());
    }
    let schema = n as f64 / t.elapsed().as_secs_f64();

    let t = Instant::now();
    for r in &records {
        std::hint::black_box(r.to_json_string());
    }
    let encode = n as f64 / t.elapsed().as_secs_f64();

    println!("records: {n}, mean wire size: {} bytes", bytes as u64 / n);
    println!("sign_record:              {sign:>10.0} records/s");
    println!("from_json_str (verify):   {verify:>10.0} records/s ({:.1} MB/s)", bytes as f64 / secs / 1e6);
    println!("check_record (schema):    {schema:>10.0} records/s");
    println!("to_json_string:           {encode:>10.0} records/s");
}
