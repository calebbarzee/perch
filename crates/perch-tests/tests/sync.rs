//! Sync with hostile peers and networks (contract 5.2, 6).
//!
//! `sync_pair(a, b, cap, filter)` runs two perch-sync hubs over in-memory
//! queues; `filter` sees every message and can drop, duplicate, reorder, or
//! rewrite it.

use perch_tests::fixture::{Log, Record};
use perch_tests::require_sut;
use perch_tests::sut::{Dir, Store, product_view, sync_pair};
use perch_tests::world::{World, key_a, product_and_listing};
use serde_json::{Value, json};

fn pass(_d: Dir, m: String) -> Vec<String> {
    vec![m]
}

fn sample() -> (World, Vec<Log>) {
    let mut w = World::new();
    let mut maker = Log::new("sync-maker");
    let mut seller = w.seller("sync-seller", "shop.example");
    product_and_listing(&mut maker, &mut seller, "shop.example", 1000);
    for i in 0..5 {
        seller.listing(&key_a(), &format!("https://shop.example/w{i}"), 1100 + i);
    }
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

fn lines(s: &Store) -> std::collections::BTreeSet<String> {
    s.export().lines().map(str::to_string).collect()
}

/// Apply `f` to every record object in a message that carries records.
fn map_records(m: &str, mut f: impl FnMut(&mut Value)) -> String {
    let Ok(mut v) = serde_json::from_str::<Value>(m) else { return m.to_string() };
    let ty = v["type"].as_str().unwrap_or("").to_string();
    if matches!(ty.as_str(), "records" | "push" | "bodies") {
        if let Some(arr) = v.get_mut("records").and_then(|r| r.as_array_mut()) {
            arr.iter_mut().for_each(&mut f);
        }
        if let Some(arr) = v.get_mut("evidence").and_then(|r| r.as_array_mut()) {
            for pair in arr.iter_mut() {
                pair.as_array_mut().unwrap().iter_mut().for_each(&mut f);
            }
        }
    }
    if ty == "evidence" {
        for pair in v["pairs"].as_array_mut().unwrap() {
            pair.as_array_mut().unwrap().iter_mut().for_each(&mut f);
        }
    }
    v.to_string()
}

#[test]
fn two_nodes_converge() {
    require_sut!(Sync);
    let (w, logs) = sample();
    let mut a = load(&w, &logs);
    let mut b = Store::new();
    let run = sync_pair(&mut a, &mut b, 10_000, &mut pass);
    assert!(!run.closed && !run.capped, "{run:?}");
    assert_eq!(a.export(), b.export());
}

#[test]
fn both_sides_contribute_and_converge() {
    require_sut!(Sync);
    let (w, logs) = sample();
    let mut a = load(&w, &logs[..2]);
    let mut b = load(&w, &logs[2..]);
    sync_pair(&mut a, &mut b, 10_000, &mut pass);
    assert_eq!(a.export(), b.export());
    assert_eq!(a.export(), load(&w, &logs).export());
}

#[test]
fn out_of_order_delivery_converges() {
    require_sut!(Sync);
    // The peer reverses the order of records inside every message it sends.
    let (w, logs) = sample();
    let mut a = load(&w, &logs);
    let mut b = Store::new();
    let mut reverse = |d: Dir, m: String| {
        if d == Dir::AtoB {
            if let Ok(mut v) = serde_json::from_str::<Value>(&m) {
                if let Some(arr) = v.get_mut("records").and_then(|r| r.as_array_mut()) {
                    arr.reverse();
                    return vec![v.to_string()];
                }
            }
        }
        vec![m]
    };
    sync_pair(&mut a, &mut b, 10_000, &mut reverse);
    assert_eq!(a.export(), b.export());
}

#[test]
fn forked_author_from_peer_truncated() {
    require_sut!(Sync);
    let (w, logs) = sample();
    let seller = &logs[1];
    let author = seller.author();
    let alt = seller.fork_at(2, "profile", json!({ "name": "evil", "role": "seller" }));
    let mut a = load(&w, &logs);
    a.ingest(&alt.wire());
    // B holds the honest branch in full.
    let mut b = load(&w, &logs);
    sync_pair(&mut a, &mut b, 10_000, &mut pass);
    assert_eq!(a.fork_seq(&author), Some(2));
    assert_eq!(b.fork_seq(&author), Some(2), "evidence pair relayed");
    assert_eq!(a.evidence(&author), b.evidence(&author));
    assert_eq!(b.head_seq(&author), 1);
    assert_eq!(a.export(), b.export());
}

#[test]
fn fork_evidence_spreads_through_a_relay() {
    require_sut!(Sync);
    // A has the fork, C has only the honest log; B relays between them.
    let (w, logs) = sample();
    let seller = &logs[1];
    let author = seller.author();
    let alt = seller.fork_at(3, "listing", json!({
        "product": key_a(), "url": "https://shop.example/x",
        "price": { "amount": 1, "currency": "EUR" }, "availability": "in_stock", "condition": "new"
    }));
    let mut a = load(&w, &logs);
    a.ingest(&alt.wire());
    let mut b = Store::new();
    let mut c = load(&w, &logs);
    sync_pair(&mut a, &mut b, 10_000, &mut pass);
    sync_pair(&mut b, &mut c, 10_000, &mut pass);
    assert_eq!(c.fork_seq(&author), Some(3));
    assert_eq!(a.export(), c.export());
}

#[test]
fn invalid_records_not_accepted_from_peer() {
    require_sut!(Sync);
    // The peer flips one signature character in every record it relays.
    let (w, logs) = sample();
    let mut a = load(&w, &logs);
    let mut b = Store::new();
    let mut corrupt = |d: Dir, m: String| {
        if d != Dir::AtoB {
            return vec![m];
        }
        vec![map_records(&m, |r| {
            if let Some(s) = r.get("sig").and_then(|s| s.as_str()) {
                let mut b = s.as_bytes().to_vec();
                b[0] = if b[0] == b'0' { b'1' } else { b'0' };
                r["sig"] = json!(String::from_utf8(b).unwrap());
            }
        })]
    };
    let run = sync_pair(&mut a, &mut b, 10_000, &mut corrupt);
    assert!(b.export().is_empty(), "no corrupted record may be stored");
    assert!(run.closed, "a peer sending invalid records is disconnected (sync-protocol 10): {run:?}");
}

#[test]
fn duplicated_messages_do_not_duplicate_records() {
    require_sut!(Sync);
    // A hostile peer sends every message twice. A duplicate `hello` is a protocol
    // violation and closes the session; duplicated data messages may also be
    // scored. Either way B stores no duplicates, holds only valid records, and a
    // later clean session converges.
    let (w, logs) = sample();
    let mut a = load(&w, &logs);
    let mut b = Store::new();
    let mut dup = |_d: Dir, m: String| vec![m.clone(), m];
    let run = sync_pair(&mut a, &mut b, 20_000, &mut dup);
    assert!(!run.capped, "duplication must not cause unbounded traffic: {run:?}");
    assert!(lines(&b).is_subset(&lines(&a)));

    // Duplicate only data messages after the handshake.
    let mut b2 = Store::new();
    let mut dup_data = |_d: Dir, m: String| {
        if m.starts_with("{\"type\":\"records\"") || m.starts_with("{\"type\":\"push\"") || m.starts_with("{\"type\":\"bodies\"") {
            vec![m.clone(), m]
        } else {
            vec![m]
        }
    };
    let run = sync_pair(&mut a, &mut b2, 20_000, &mut dup_data);
    assert!(!run.capped, "{run:?}");
    let lb = lines(&b2);
    assert_eq!(b2.export().lines().count(), lb.len(), "no duplicate lines");
    assert!(lb.is_subset(&lines(&a)));
    sync_pair(&mut a, &mut b2, 10_000, &mut pass);
    assert_eq!(a.export(), b2.export());
}

#[test]
fn dropped_messages_recovered_on_reconnect() {
    require_sut!(Sync);
    // Every third message from A is lost; a later clean session completes sync.
    let (w, logs) = sample();
    let mut a = load(&w, &logs);
    let mut b = Store::new();
    let mut n = 0;
    let mut lossy = |d: Dir, m: String| {
        if d == Dir::AtoB {
            n += 1;
            if n % 3 == 0 {
                return vec![];
            }
        }
        vec![m]
    };
    sync_pair(&mut a, &mut b, 10_000, &mut lossy);
    assert!(lines(&b).is_subset(&lines(&a)));
    sync_pair(&mut a, &mut b, 10_000, &mut pass);
    assert_eq!(a.export(), b.export());
}

#[test]
fn stripped_bodies_restored_from_honest_peer() {
    require_sut!(Sync);
    // Peer A strips every body. B keeps the headers, then gets bodies from C.
    let (w, logs) = sample();
    let mut a = load(&w, &logs);
    let mut b = Store::new();
    let mut strip = |d: Dir, m: String| {
        if d == Dir::AtoB {
            vec![map_records(&m, |r| {
                r.as_object_mut().unwrap().remove("body");
            })]
        } else {
            vec![m]
        }
    };
    sync_pair(&mut a, &mut b, 10_000, &mut strip);
    let some: Record = logs[1].records[1].clone();
    assert!(b.has(&some.id), "header-only records keep the log");
    assert!(!b.has_body(&some.id));
    let mut c = load(&w, &logs);
    sync_pair(&mut c, &mut b, 10_000, &mut pass);
    assert!(b.has_body(&some.id), "want_bodies fetched the body from C");
    assert_eq!(b.export(), c.export());
}

#[test]
fn forged_forked_hint_ignored() {
    require_sut!(Sync);
    // A marks every head as forked without evidence. B must not mark anyone forked.
    let (w, logs) = sample();
    let mut a = load(&w, &logs);
    let mut b = Store::new();
    let mut forge = |d: Dir, m: String| {
        if d == Dir::AtoB {
            if let Ok(mut v) = serde_json::from_str::<Value>(&m) {
                if v["type"] == "heads" {
                    for h in v["heads"].as_array_mut().unwrap() {
                        h["forked"] = json!(true);
                    }
                    return vec![v.to_string()];
                }
            }
        }
        vec![m]
    };
    sync_pair(&mut a, &mut b, 10_000, &mut forge);
    assert_eq!(a.export(), b.export(), "records still sync");
    for l in &logs {
        assert_eq!(b.fork_seq(&l.author()), None, "forked hint alone must not fork {}", l.author());
    }
}

#[test]
fn inflated_head_claim_does_not_block_other_authors() {
    require_sut!(Sync);
    // A claims the reviewer's log has 1,000,000 records. B must still sync every
    // log, and must not invent records.
    let (w, logs) = sample();
    let mut a = load(&w, &logs);
    let mut b = Store::new();
    let victim = logs[2].author();
    let mut inflate = |d: Dir, m: String| {
        if d == Dir::AtoB {
            if let Ok(mut v) = serde_json::from_str::<Value>(&m) {
                if v["type"] == "heads" {
                    for h in v["heads"].as_array_mut().unwrap() {
                        if h["author"] == json!(victim) {
                            h["seq"] = json!(1_000_000);
                            h["id"] = json!("f".repeat(64));
                        }
                    }
                    return vec![v.to_string()];
                }
            }
        }
        vec![m]
    };
    let run = sync_pair(&mut a, &mut b, 10_000, &mut inflate);
    assert!(!run.capped, "an unprovable head must not cause endless requests: {run:?}");
    for l in &logs {
        assert_eq!(b.head_seq(&l.author()), l.records.len() as i64, "author {}", l.author());
    }
}

#[test]
fn unsolicited_push_of_forked_branch_is_evidence_not_history() {
    require_sut!(Sync);
    // A pushes a conflicting record for an author B already holds. B treats it as
    // fork evidence and truncates; it never stores the conflicting branch as history.
    let (w, logs) = sample();
    let seller = &logs[1];
    let alt = seller.fork_at(2, "profile", json!({ "name": "evil", "role": "seller" }));
    let mut a = load(&w, &logs);
    let mut b = load(&w, &logs);
    let mut sent = false;
    let alt_wire = alt.to_value();
    let mut inject = |d: Dir, m: String| {
        if d == Dir::AtoB && !sent && m.starts_with("{\"type\":\"hello\"") {
            sent = true;
            let push = json!({ "type": "push", "records": [alt_wire.clone()], "evidence": [] }).to_string();
            return vec![m, push];
        }
        vec![m]
    };
    sync_pair(&mut a, &mut b, 10_000, &mut inject);
    assert_eq!(b.fork_seq(&seller.author()), Some(2));
    assert_eq!(b.head_seq(&seller.author()), 1);
    let _ = &mut a;
}

#[test]
fn removed_body_not_restored_by_sync() {
    require_sut!(Sync);
    // B removed a review body (legal notice). Syncing with a peer that still has it
    // must not bring the body back (contract 5.3).
    let (w, logs) = sample();
    let review = logs[2].records.last().unwrap().clone();
    let mut a = load(&w, &logs);
    let mut b = load(&w, &logs);
    assert_eq!(b.remove_body(&review.id), Ok(true));
    sync_pair(&mut a, &mut b, 10_000, &mut pass);
    assert!(b.has(&review.id));
    assert!(!b.has_body(&review.id), "removal list must hold against peers");
    assert!(a.has_body(&review.id), "the peer keeps its own copy");
}

#[test]
fn synced_nodes_compute_same_views() {
    require_sut!(Views);
    let (w, logs) = sample();
    let mut a = load(&w, &logs);
    let mut b = Store::new();
    sync_pair(&mut a, &mut b, 10_000, &mut pass);
    let p = w.policy();
    assert_eq!(product_view(&a, &p, &key_a()), product_view(&b, &p, &key_a()));
}

#[test]
fn many_equivocations_converge_in_random_graphs() {
    require_sut!(Sync);
    // 12 peers, random graphs, 6 authors; 3 authors equivocate at different
    // peers in different seeds. Every seed must converge to one state.
    for seed in 1..=8u64 {
        let mut sim = perch_sync::sim::Sim::new(12, seed);
        sim.add_authors(6, seed as u8);
        sim.random_graph(6);
        for round in 0..4 {
            for a in 0..6 {
                sim.publish((a + round) % 12, a);
            }
        }
        sim.equivocate(0, 0, 11);
        sim.equivocate(1, 3, 7);
        sim.publish(5, 2);
        sim.equivocate(2, 5, 5); // both versions at the same peer
        sim.run(2_000_000);
        sim.refresh_all();
        sim.run(2_000_000);
        let state = sim.check_converged(&sim.all()).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        assert_eq!(state.pairs.len(), 3, "seed {seed}: three forked authors");
    }
}

#[test]
fn heads_paging_syncs_more_than_one_page() {
    require_sut!(Sync);
    // 4,200 authors exceed one heads page (4,096).
    let mut a = Store::new();
    for i in 0..4_200 {
        let mut l = Log::new(&format!("page-{i}"));
        a.ingest(&l.profile("P", "buyer").wire());
    }
    let mut b = Store::new();
    let run = sync_pair(&mut a, &mut b, 100_000, &mut pass);
    assert!(!run.closed && !run.capped, "{run:?}");
    assert_eq!(b.export().lines().count(), 4_200);
}
