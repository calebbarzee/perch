//! Live tests of admission tiers (ARCHITECTURE 9, docs/api.md section 6).

mod common;

use common::*;
use perch_core::{AuthorId, Keypair, Record, RecordId, Unsigned, sign_record};
use serde_json::{Value, json};

fn signed(k: &Keypair, seq: u64, prev: Option<RecordId>, kind: &str, body: Value) -> Record {
    let body = body.as_object().unwrap().clone();
    sign_record(k, Unsigned { seq, prev, ts: 1_000 + seq, kind: kind.into(), replaces: None, body }).unwrap()
}

fn profiles(k: &Keypair, n: u64) -> Vec<Record> {
    let mut out: Vec<Record> = Vec::new();
    for s in 1..=n {
        let prev = out.last().map(|r| r.id());
        out.push(signed(k, s, prev, "profile", json!({ "name": format!("key {s}"), "role": "buyer" })));
    }
    out
}

fn jsonl(rs: &[Record]) -> String {
    rs.iter().map(|r| r.to_json_string()).collect::<Vec<_>>().join("\n")
}

fn outcomes(v: &Value) -> Vec<String> {
    v["results"].as_array().unwrap().iter().map(|r| r["outcome"].as_str().unwrap().to_string()).collect()
}

fn log_len(n: &NodeProc, a: &AuthorId) -> usize {
    n.get(&format!("/v1/authors/{a}/log")).json().as_array().unwrap().len()
}

/// Same rank as the node: sha256(author bytes ‖ u64be(floor(day / 30))).
fn rank(a: &AuthorId) -> [u8; 32] {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    let mut b = a.as_bytes().to_vec();
    b.extend_from_slice(&(now / 86_400_000 / 30).to_be_bytes());
    perch_core::sha256(&b)
}

#[test]
fn h_tier3_per_author_limit_and_node_budget() {
    let dir = tmpdir("tier3");
    // Per-author limit.
    let n = spawn(&dir.join("a"), &["--tier3-records".into(), "3".into()]);
    let k = Keypair::from_seed([90; 32]);
    let v = n.post("/v1/records", &jsonl(&profiles(&k, 4))).json();
    assert_eq!(outcomes(&v), ["inserted", "inserted", "inserted", "admission"]);
    assert_eq!(log_len(&n, &k.author()), 3);
    drop(n);

    // Node budget: 30 unverified keys with one record each; room for about 10.
    let n = spawn(&dir.join("b"), &["--tier3-budget".into(), "6000".into()]);
    let keys: Vec<Keypair> = (0..30u8).map(|i| Keypair::from_seed([100 + i; 32])).collect();
    let records: Vec<Record> = keys.iter().map(|k| profiles(k, 1).remove(0)).collect();
    let v = n.post("/v1/records", &jsonl(&records)).json();
    let refused = v["results"].as_array().unwrap().iter().filter(|r| r["outcome"] == "admission").count();
    assert!(refused > 0, "{v}");
    for r in v["results"].as_array().unwrap().iter().filter(|r| r["outcome"] == "admission") {
        assert!(r["error"].as_str().unwrap().contains("budget"), "{r}");
    }
    let mut by_rank: Vec<AuthorId> = keys.iter().map(|k| k.author()).collect();
    by_rank.sort_by_key(|a| (rank(a), *a));
    let kept: Vec<bool> = by_rank.iter().map(|a| log_len(&n, a) == 1).collect();
    let k = kept.iter().take_while(|x| **x).count();
    assert!(k > 0 && k < 30, "some but not all kept: {k}");
    assert!(kept[k..].iter().all(|x| !x), "kept authors are a prefix of the rank order: {kept:?}");
    let st = n.status();
    assert_eq!(st["store"]["authors"], k);
}

#[test]
fn i_tier1_limit_applies_per_person() {
    let dir = tmpdir("tier1");
    let att = Keypair::from_seed([95; 32]);
    let n = spawn(&dir.join("a"), &["--attestor".into(), att.author().to_hex(), "--tier1-records".into(), "5".into()]);
    let (k1, k2, k3) = (Keypair::from_seed([96; 32]), Keypair::from_seed([97; 32]), Keypair::from_seed([98; 32]));
    let expires = 4_000_000_000_000u64;
    let attest = |seq: u64, prev: Option<RecordId>, subject: &Keypair, person: &str| {
        signed(
            &att,
            seq,
            prev,
            "attest.identity",
            json!({ "subject": subject.author().to_hex(), "person": person.repeat(32), "level": "person", "expires": expires }),
        )
    };
    let a1 = attest(1, None, &k1, "ab");
    let a2 = attest(2, Some(a1.id()), &k2, "ab");
    let a3 = attest(3, Some(a2.id()), &k3, "cd");
    let v = n.post("/v1/records", &jsonl(&[a1, a2, a3])).json();
    assert_eq!(outcomes(&v), ["inserted", "inserted", "inserted"]);
    // One person with two keys shares 5 records.
    let v = n.post("/v1/records", &jsonl(&profiles(&k1, 3))).json();
    assert_eq!(outcomes(&v), ["inserted", "inserted", "inserted"]);
    let v = n.post("/v1/records", &jsonl(&profiles(&k2, 3))).json();
    assert_eq!(outcomes(&v), ["inserted", "inserted", "admission"], "{v}");
    assert_eq!(log_len(&n, &k1.author()) + log_len(&n, &k2.author()), 5);
    // Another person has its own 5.
    let v = n.post("/v1/records", &jsonl(&profiles(&k3, 6))).json();
    assert_eq!(outcomes(&v), ["inserted", "inserted", "inserted", "inserted", "inserted", "admission"]);
}
