//! Sync with hostile peers (contract 6, 5.2).

use perch_tests::fixture::Log;
use perch_tests::require_sut;
use perch_tests::sut::{Dir, Store, product_view, sync_pair};
use perch_tests::world::{World, key_a, product_and_listing};
use serde_json::json;

fn passthrough() -> impl FnMut(Dir, String) -> Vec<String> {
    |_d, m| vec![m]
}

fn sample() -> (World, Vec<Log>) {
    let mut w = World::new();
    let mut maker = Log::new("sync-maker");
    let mut seller = w.seller("sync-seller", "shop.example");
    product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    let mut r = w.verified("sync-rev", "sync-rev");
    r.review(&key_a(), 4, None);
    (w, vec![maker, seller, r])
}

fn load(w: &World, logs: &[Log]) -> Store {
    let refs: Vec<&Log> = logs.iter().collect();
    let mut s = Store::new();
    for l in w.lines(&refs) {
        s.ingest(&l);
    }
    s
}

#[test]
fn two_nodes_converge_to_same_views() {
    require_sut!();
    let (w, logs) = sample();
    let mut a = load(&w, &logs);
    let mut b = Store::new();
    sync_pair(&mut a, &mut b, 100, &mut passthrough());
    assert_eq!(a.export(), b.export());
    let p = w.policy();
    assert_eq!(product_view(&a, &p, &key_a()), product_view(&b, &p, &key_a()));
}

#[test]
fn out_of_order_delivery_converges() {
    require_sut!();
    // The peer reverses the order of records inside every message it sends.
    let (w, logs) = sample();
    let mut a = load(&w, &logs);
    let mut b = Store::new();
    let mut reverse = |d: Dir, m: String| {
        if d == Dir::AtoB {
            if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&m) {
                if let Some(arr) = v.get_mut("records").and_then(|r| r.as_array_mut()) {
                    arr.reverse();
                    return vec![v.to_string()];
                }
            }
        }
        vec![m]
    };
    sync_pair(&mut a, &mut b, 200, &mut reverse);
    assert_eq!(a.export(), b.export());
}

#[test]
fn forked_author_from_peer_truncated() {
    require_sut!();
    let (w, mut logs) = sample();
    let seller = &logs[1];
    let alt = seller.fork_at(2, "profile", json!({ "name": "evil", "role": "seller" }));
    let mut a = load(&w, &logs);
    a.ingest(&alt.wire());
    let mut b = load(&w, &logs[..0]);
    // B already holds the honest branch.
    for r in &logs[1].records {
        b.ingest(&r.wire());
    }
    sync_pair(&mut a, &mut b, 200, &mut passthrough());
    let author = logs[1].author();
    assert_eq!(a.fork_seq(&author), Some(2));
    assert_eq!(b.fork_seq(&author), Some(2), "evidence pair relayed");
    assert_eq!(a.evidence(&author), b.evidence(&author));
    assert_eq!(b.head_seq(&author), 1);
    logs.clear();
}

#[test]
fn invalid_records_not_accepted_from_peer() {
    require_sut!();
    // The peer flips one signature byte in every record it relays.
    let (w, logs) = sample();
    let mut a = load(&w, &logs);
    let mut b = Store::new();
    let mut corrupt = |d: Dir, m: String| {
        if d == Dir::AtoB {
            if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&m) {
                if let Some(arr) = v.get_mut("records").and_then(|r| r.as_array_mut()) {
                    for r in arr.iter_mut() {
                        if let Some(s) = r.get("sig").and_then(|s| s.as_str()) {
                            let mut b = s.as_bytes().to_vec();
                            b[0] = if b[0] == b'0' { b'1' } else { b'0' };
                            r["sig"] = json!(String::from_utf8(b).unwrap());
                        }
                    }
                    return vec![v.to_string()];
                }
            }
        }
        vec![m]
    };
    sync_pair(&mut a, &mut b, 200, &mut corrupt);
    assert!(b.export().is_empty(), "no corrupted record may be stored");
}

#[test]
fn replayed_messages_do_not_duplicate() {
    require_sut!();
    let (w, logs) = sample();
    let mut a = load(&w, &logs);
    let mut b = Store::new();
    let mut dup = |_d: Dir, m: String| vec![m.clone(), m];
    sync_pair(&mut a, &mut b, 400, &mut dup);
    assert_eq!(a.export(), b.export());
}
