//! Client behavior with perch-core signing and the store: publish continuity,
//! receipts, verification, tamper rejection, forks, gaps, and the publish guard.

use perch_wasm::client::{Client, split_records};
use perch_wasm::guard::{CLAIM_TIMEOUT_MS, HeadRef};
use serde_json::{Value, json};

const T: u64 = 1_760_000_000_000;

fn publish(c: &mut Client, kind: &str, body: Value) -> (String, Value) {
    let s = c.publish(kind, &body.to_string(), None, Some(T), T).expect("publish");
    (s.id, serde_json::from_str(&s.wire).unwrap())
}

#[test]
fn publish_continues_the_log() {
    let mut c = Client::new(false);
    let a = c.generate_key([3; 32]);
    let (id1, r1) = publish(&mut c, "profile", json!({ "name": "Ada", "role": "buyer" }));
    assert_eq!(r1["seq"], 1);
    assert_eq!(r1["prev"], Value::Null);
    assert_eq!(r1["author"], json!(a));
    let (_, r2) = publish(&mut c, "profile", json!({ "name": "Ada L.", "role": "buyer" }));
    assert_eq!(r2["seq"], 2);
    assert_eq!(r2["prev"], json!(id1));
    let backup: Value = serde_json::from_str(&c.export_key().unwrap()).unwrap();
    assert_eq!(backup["head"]["seq"], 2);
}

#[test]
fn ts_never_decreases() {
    let mut c = Client::new(false);
    c.generate_key([4; 32]);
    c.publish("profile", &json!({ "name": "A", "role": "buyer" }).to_string(), None, Some(T + 10), T).unwrap();
    let s = c.publish("profile", &json!({ "name": "B", "role": "buyer" }).to_string(), None, Some(T), T).unwrap();
    let r: Value = serde_json::from_str(&s.wire).unwrap();
    assert_eq!(r["ts"], json!(T + 10));
}

#[test]
fn invalid_body_is_refused_before_publishing() {
    let mut c = Client::new(false);
    c.generate_key([5; 32]);
    let err = c.publish("review", &json!({ "rating": 9 }).to_string(), None, Some(T), T).unwrap_err();
    assert!(err.contains("review"), "{err}");
    assert!(c.own_local_head().is_none());
}

#[test]
fn tampered_records_are_rejected() {
    let mut c = Client::new(false);
    c.generate_key([6; 32]);
    let (id, rec) = publish(&mut c, "profile", json!({ "name": "Ada", "role": "buyer" }));
    let mut other = Client::new(false);
    let ok = other.ingest_lines(&[rec.to_string()]);
    assert_eq!(ok.inserted, 1);

    let mut bad = rec.clone();
    bad["body"]["name"] = json!("Eve");
    let mut sig = rec.clone();
    let s = sig["sig"].as_str().unwrap().to_string();
    sig["sig"] = json!(format!("{}{}", if s.starts_with('a') { "b" } else { "a" }, &s[1..]));
    let mut third = Client::new(false);
    let r = third.ingest_lines(&[bad.to_string(), sig.to_string()]);
    assert_eq!(r.rejected, 2);
    assert_eq!(r.inserted, 0);
    let v = third.backend.verify_record(&bad.to_string());
    assert_eq!(v["valid"], json!(false));
    assert_eq!(v["id"], json!(id));
    assert_eq!(c.backend.verify_record(&rec.to_string())["valid"], json!(true));
}

#[test]
fn receipts_verify_under_the_listing_seller() {
    let mut seller = Client::new(false);
    seller.generate_key([7; 32]);
    let (lid, listing) = publish(
        &mut seller,
        "listing",
        json!({ "product": "gtin:04006381333931", "url": "https://shop.example/p/1",
                "price": { "amount": 4999, "currency": "EUR" }, "availability": "in_stock", "condition": "new" }),
    );
    let mut buyer = Client::new(false);
    let b = buyer.generate_key([8; 32]);
    buyer.ingest_lines(&[listing.to_string()]);
    let receipt = seller.issue_receipt(&lid, &b, T, [9; 32]).unwrap();
    let ok = buyer.backend.verify_receipt(&receipt.to_string());
    assert_eq!(ok["valid"], json!(true), "{ok}");
    assert_eq!(ok["seller"], json!(seller.author().unwrap()));

    let mut forged = receipt.clone();
    forged["purchased_at"] = json!(T + 1);
    assert_eq!(buyer.backend.verify_receipt(&forged.to_string())["valid"], json!(false));
    let unknown = Client::new(false).backend.verify_receipt(&receipt.to_string());
    assert!(unknown["error"].as_str().unwrap().contains("not in your local data"));
}

#[test]
fn gaps_are_held_and_forks_detected() {
    let mut a = Client::new(false);
    a.generate_key([10; 32]);
    let (_, r1) = publish(&mut a, "profile", json!({ "name": "A", "role": "buyer" }));
    let (_, r2) = publish(&mut a, "profile", json!({ "name": "B", "role": "buyer" }));
    let (_, r3) = publish(&mut a, "profile", json!({ "name": "C", "role": "buyer" }));

    let mut c = Client::new(false);
    let r = c.ingest_lines(&[r3.to_string(), r2.to_string()]);
    assert_eq!(r.gap, 2);
    let r = c.ingest_lines(&[r1.to_string()]);
    assert_eq!(r.inserted, 1);
    assert_eq!(c.backend.head(a.author().unwrap()).unwrap().seq, 3);

    // Same key, different record at seq 2: a fork. The log is cut to seq 1.
    let id1 = a.backend.verify_record(&r1.to_string())["id"].as_str().unwrap().to_string();
    let s = a.backend.sign_record(&[10; 32], "profile", &json!({ "name": "X", "role": "buyer" }).to_string(), 2, Some(&id1), T + 5, None);
    let fork = s.unwrap();
    let r = c.ingest_lines(&[fork.wire]);
    assert_eq!(r.fork, 1);
    assert_eq!(c.backend.head(a.author().unwrap()).unwrap().seq, 1);
}

#[test]
fn imported_key_waits_for_every_node_then_sees_the_highest_head() {
    let mut a = Client::new(false);
    let secret_backup = {
        a.generate_key([11; 32]);
        a.export_key().unwrap()
    };
    let (_, r1) = publish(&mut a, "profile", json!({ "name": "A", "role": "buyer" }));

    let mut b = Client::new(false);
    b.guard.add_node("ws://n1");
    b.guard.add_node("ws://n2");
    b.import_key(&secret_backup).unwrap();
    assert!(!b.publish_status(T).ready);
    // n1 reports seq 1; n2 reports nothing.
    let id1 = a.backend.verify_record(&r1.to_string())["id"].as_str().unwrap().to_string();
    b.guard.node_reported("ws://n1", Some(HeadRef { seq: 1, id: id1 }), T);
    b.guard.node_checked("ws://n1");
    b.guard.node_checked("ws://n2");
    // Device lacks record 1.
    let s = b.publish_status(T);
    assert!(!s.ready, "{s:?}");
    b.ingest_lines(&[r1.to_string()]);
    assert!(b.publish_status(T).ready);
    let s = b.publish("profile", &json!({ "name": "A2", "role": "buyer" }).to_string(), None, Some(T), T).unwrap();
    let r: Value = serde_json::from_str(&s.wire).unwrap();
    assert_eq!(r["seq"], 2);
}

#[test]
fn json_array_input() {
    let lines = split_records(r#"[{"a":1}]"#).unwrap();
    assert_eq!(lines.len(), 1);
}

#[test]
fn currencies_come_from_core() {
    let c = Client::new(false);
    let cur = c.backend.currencies();
    let iqd = cur.as_array().unwrap().iter().find(|x| x["code"] == "IQD").unwrap();
    assert_eq!(iqd["minor_units"], json!(3));
}

#[test]
fn fake_head_from_a_node_cannot_block_publishing_forever() {
    let mut c = Client::new(false);
    c.guard.add_node("ws://evil.example/v1/sync");
    c.generate_key([12; 32]);
    let (_, _r1) = publish(&mut c, "profile", json!({ "name": "A", "role": "buyer" }));
    // The node claims a head far above anything the user signed.
    c.guard.node_reported("ws://evil.example/v1/sync", Some(HeadRef { seq: 1_000_000, id: "f".repeat(64) }), T);
    let s = c.publish_status(T + 1);
    assert!(!s.ready);
    assert!(s.reason.unwrap().contains("evil.example"));
    // The claim never raises the proven head.
    let backup: Value = serde_json::from_str(&c.export_key().unwrap()).unwrap();
    assert_eq!(backup["head"]["seq"], 1);
    // Without proof the claim is ignored after the timeout, and named.
    let s = c.publish_status(T + CLAIM_TIMEOUT_MS);
    assert!(s.ready, "{s:?}");
    assert!(s.reason.as_deref().unwrap().contains("cannot prove"));
    let p = c.publish("profile", &json!({ "name": "B", "role": "buyer" }).to_string(), None, Some(T), T + CLAIM_TIMEOUT_MS).unwrap();
    let r: Value = serde_json::from_str(&p.wire).unwrap();
    assert_eq!(r["seq"], 2);
}

/// Web Crypto Ed25519 in some engines accepts the identity point as a public
/// key with R = identity and S = 0 for any message. perch-core must reject it.
#[test]
fn identity_point_forgery_is_rejected() {
    let identity = format!("01{}", "00".repeat(31));
    let sig = format!("{identity}{}", "00".repeat(32));
    let body = json!({ "name": "Forged", "role": "seller" });
    let body_c = perch_core::to_canonical_json(&body).unwrap();
    let body_hash = perch_core::ids::encode_hex(&perch_core::sha256(body_c.as_bytes()));
    let rec = json!({
        "v": 1, "author": identity, "seq": 1, "prev": null, "ts": T, "kind": "profile",
        "body_hash": body_hash, "sig": sig, "body": body,
    });
    let mut c = Client::new(false);
    let r = c.ingest_lines(&[rec.to_string()]);
    assert_eq!(r.rejected, 1, "{:?}", r.errors);
    assert_eq!(r.inserted, 0);
    assert_eq!(c.backend.verify_record(&rec.to_string())["valid"], json!(false));
    let mut sparse = Client::new(true);
    assert_eq!(sparse.ingest_lines(&[rec.to_string()]).rejected, 1);
}
