use super::*;
use serde_json::json;

fn m(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

#[test]
fn profile_ok() {
    let b = parse_body("profile", &m(json!({"name": "A", "role": "seller", "extra": 1}))).unwrap();
    assert!(matches!(b, Body::Profile(_)));
}
