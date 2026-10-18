//! Writes the small demo dataset and its policy, for the Node API test:
//! `cargo run -p perch-wasm --example demo_jsonl -- out.jsonl policy.json`

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (out, policy_out) = (&args[1], &args[2]);
    let d = perch_demo::generate_demo(7, perch_demo::Size::Small);
    std::fs::write(out, perch_core::to_jsonl(d.records.iter())).expect("write jsonl");
    let mut p = serde_json::to_value(perch_views::default_policy(d.now, &d.attestors)).unwrap();
    p["now"] = serde_json::Value::Null;
    std::fs::write(policy_out, serde_json::to_string_pretty(&p).unwrap()).expect("write policy");
}
