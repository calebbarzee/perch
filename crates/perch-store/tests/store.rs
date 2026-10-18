use perch_core::{AuthorId, EvidencePair, Keypair, Record, RecordId, Unsigned, sign_record};
use perch_store::{AdmissionLimit, Change, Head, HoldLimits, Insert, MemoryStore, Mode, Reject, Store, StoreError};
use serde_json::{Map, Value, json};

fn obj(v: Value) -> Map<String, Value> {
    v.as_object().unwrap().clone()
}

fn kp(n: u8) -> Keypair {
    Keypair::from_seed([n; 32])
}

fn rec(k: &Keypair, seq: u64, prev: Option<RecordId>, ts: u64, kind: &str, body: Value) -> Record {
    sign_record(k, Unsigned { seq, prev, ts, kind: kind.into(), replaces: None, body: obj(body) }).unwrap()
}

fn rep(k: &Keypair, seq: u64, prev: Option<RecordId>, kind: &str, replaces: RecordId, body: Value) -> Record {
    sign_record(k, Unsigned { seq, prev, ts: 100 + seq, kind: kind.into(), replaces: Some(replaces), body: obj(body) })
        .unwrap()
}

/// A chain of `n` simple records; `tag` makes branches differ.
fn chain(k: &Keypair, n: u64, tag: u64) -> Vec<Record> {
    let mut out: Vec<Record> = Vec::new();
    for s in 1..=n {
        let prev = out.last().map(|r| r.id());
        out.push(rec(k, s, prev, 100 + s, "note", json!({ "n": s, "t": tag })));
    }
    out
}

/// A branch that continues `base[..from-1]` with different records from seq `from`.
fn branch(k: &Keypair, base: &[Record], from: u64, len: u64, tag: u64) -> Vec<Record> {
    let mut out: Vec<Record> = Vec::new();
    let mut prev = if from > 1 { Some(base[(from - 2) as usize].id()) } else { None };
    for s in from..from + len {
        let r = rec(k, s, prev, 100 + s, "note", json!({ "n": s, "t": tag }));
        prev = Some(r.id());
        out.push(r);
    }
    out
}

fn state(s: &MemoryStore) -> (Vec<Head>, Vec<Option<[RecordId; 2]>>, Vec<RecordId>) {
    let heads = s.heads(None, usize::MAX);
    let pairs = s.authors().iter().map(|a| s.evidence(a).map(|p| p.ids())).collect();
    let mut ids: Vec<RecordId> = s.export_records().map(|r| r.id()).collect();
    ids.sort();
    (heads, pairs, ids)
}

#[test]
fn prefix_duplicate_gap_and_drain() {
    let k = kp(1);
    let a = k.author();
    let c = chain(&k, 5, 0);
    let mut s = MemoryStore::new();
    s.set_track_changes(true);
    assert_eq!(s.insert(c[0].clone()), Insert::Inserted);
    assert_eq!(s.insert(c[0].clone()), Insert::Duplicate);
    assert_eq!(s.insert(c[3].clone()), Insert::Gap);
    assert_eq!(s.insert(c[2].clone()), Insert::Gap);
    assert_eq!(s.head(&a).unwrap().seq, 1);
    assert_eq!(s.insert(c[1].clone()), Insert::Inserted);
    assert_eq!(s.head(&a).unwrap().seq, 4, "held records drained");
    assert_eq!(s.held_count(), 0);
    let changes = s.drain_changes();
    let inserted: Vec<u64> =
        changes.iter().filter_map(|c| if let Change::Inserted(r) = c { Some(r.seq()) } else { None }).collect();
    assert_eq!(inserted, vec![1, 2, 3, 4]);
    assert_eq!(s.next_position(&a), (5, Some(c[3].id())));
    assert_eq!(s.log(&a, 2, 2).iter().map(|r| r.seq()).collect::<Vec<_>>(), vec![2, 3]);
    assert_eq!(s.len(), 4);
}

#[test]
fn ts_must_not_decrease() {
    let k = kp(2);
    let r1 = rec(&k, 1, None, 500, "note", json!({}));
    let r2 = rec(&k, 2, Some(r1.id()), 499, "note", json!({}));
    let r2ok = rec(&k, 2, Some(r1.id()), 500, "note", json!({"x": 1}));
    let mut s = MemoryStore::new();
    s.insert(r1);
    assert_eq!(s.insert(r2), Insert::Rejected(Reject::TsDecrease { prev_ts: 500, ts: 499 }));
    assert_eq!(s.insert(r2ok), Insert::Inserted);
}

#[test]
fn fork_same_seq_truncates_and_keeps_lowest_pair() {
    let k = kp(3);
    let a = k.author();
    let x = chain(&k, 6, 0);
    let y = branch(&k, &x, 4, 3, 1);
    let mut s = MemoryStore::new();
    for r in &x {
        s.insert(r.clone());
    }
    assert_eq!(s.head(&a).unwrap().seq, 6);
    // y6 conflicts with x6 (same seq) and with x5 (y6.prev = y5 ≠ x5): fork at 5.
    assert_eq!(s.insert(y[2].clone()), Insert::Fork { pair_changed: true });
    assert_eq!(s.fork_seq(&a), Some(5));
    assert_eq!(s.head(&a).unwrap().seq, 4);
    // A lower conflict at seq 4 replaces it.
    assert_eq!(s.insert(y[0].clone()), Insert::Fork { pair_changed: true });
    assert_eq!(s.fork_seq(&a), Some(4));
    let h = s.head(&a).unwrap();
    assert_eq!(h.seq, 3);
    assert!(h.forked());
    // Record at seq 5 of branch y cannot lower the pair (same-seq pair at 4 is lowest).
    assert_eq!(s.insert(y[1].clone()), Insert::Fork { pair_changed: false });
    // Records above the fork are not stored.
    assert_eq!(s.insert(x[4].clone()), Insert::Fork { pair_changed: false });
    let p = s.evidence(&a).unwrap();
    let mut want = [x[3].id(), y[0].id()];
    want.sort();
    assert_eq!(p.ids(), want);
    assert!(!p.a().has_body() && !p.b().has_body(), "evidence stored without bodies");
    assert!(s.get(&x[3].id()).is_some(), "evidence records are retrievable");
    assert_eq!(s.len(), 3);
    assert_eq!(s.forked_authors(), vec![a]);
}

#[test]
fn fork_prev_mismatch_and_late_prefix() {
    let k = kp(4);
    let a = k.author();
    let x = chain(&k, 3, 0);
    let y = branch(&k, &x, 2, 2, 1); // y2.prev = x1, y3.prev = y2
    let mut s = MemoryStore::new();
    s.insert(x[0].clone());
    s.insert(x[1].clone());
    // y3 at seq 3 has prev y2 ≠ x2: type 2 evidence at seq 2.
    assert_eq!(s.insert(y[1].clone()), Insert::Fork { pair_changed: true });
    assert_eq!(s.fork_seq(&a), Some(2));
    // y2 forms the same-seq pair at 2, which is lower.
    assert_eq!(s.insert(y[0].clone()), Insert::Fork { pair_changed: true });
    let p = s.evidence(&a).unwrap();
    assert_eq!(p.a().seq(), 2);
    assert_eq!(p.b().seq(), 2);
}

#[test]
fn insert_evidence_without_prefix_then_prefix_lowers_it() {
    let k = kp(5);
    let a = k.author();
    let x = chain(&k, 6, 0);
    let y = branch(&k, &x, 5, 1, 1);
    let mut s = MemoryStore::new();
    assert_eq!(s.insert_evidence(y[0].clone(), x[4].clone()), Insert::Fork { pair_changed: true });
    assert_eq!(s.head(&a).unwrap().seq, 0);
    assert_eq!(s.fork_seq(&a), Some(5));
    for r in &x[..4] {
        s.insert(r.clone());
    }
    assert_eq!(s.head(&a).unwrap().seq, 4);
    assert_eq!(s.fork_seq(&a), Some(5));
    // A non-pair is refused.
    assert!(matches!(s.insert_evidence(x[0].clone(), x[1].clone()), Insert::Rejected(Reject::NotEvidence(_))));
    // A higher pair does not replace the stored one.
    let z = branch(&k, &x, 6, 1, 2);
    assert_eq!(s.insert_evidence(z[0].clone(), x[5].clone()), Insert::Fork { pair_changed: false });
    assert_eq!(s.fork_seq(&a), Some(5));
}

#[test]
fn bodies_fill_remove_and_control_kinds() {
    let k = kp(6);
    let a = k.author();
    let r1 = rec(&k, 1, None, 1, "note", json!({"text": "hello"}));
    let r2 = rec(&k, 2, Some(r1.id()), 2, "retire", json!({}));
    let r3 = rec(&k, 3, Some(r2.id()), 3, "note", json!({"text": "x"}));
    let mut s = MemoryStore::new();
    s.set_track_changes(true);
    assert_eq!(s.insert(r1.without_body()), Insert::Inserted);
    assert_eq!(s.missing_bodies(None, 10), vec![r1.id()]);
    assert_eq!(s.insert(r1.clone()), Insert::BodyFilled);
    assert!(s.get(&r1.id()).unwrap().has_body());
    assert!(s.missing_bodies(None, 10).is_empty());
    s.insert(r2.clone());
    assert!(matches!(s.remove_body(&r2.id()), Err(StoreError::ControlKind(_))));
    assert_eq!(s.remove_body(&r1.id()).unwrap(), true);
    assert!(!s.get(&r1.id()).unwrap().has_body());
    assert!(s.is_removed(&r1.id()));
    assert!(s.missing_bodies(None, 10).is_empty(), "removed IDs are not requested");
    assert_eq!(s.bodyless_count(), 1);
    assert_eq!(s.insert(r1.clone()), Insert::Duplicate, "removed body is refused");
    // Removal list applies before arrival.
    s.add_removed([r3.id()]);
    assert_eq!(s.insert(r3.clone()), Insert::Inserted);
    assert!(!s.get(&r3.id()).unwrap().has_body());
    assert_eq!(s.head(&a).unwrap().seq, 3);
}

#[test]
fn indexes() {
    let seller = kp(7);
    let att = kp(8);
    let p1 = rec(&seller, 1, None, 1, "product", json!({"name": "Kettle", "brand": "Acme", "gtin": "4006381333931"}));
    let p2 = rec(&seller, 2, Some(p1.id()), 2, "product", json!({"name": "Plain thing", "brand": "Acme"}));
    let l1 = rec(
        &seller,
        3,
        Some(p2.id()),
        3,
        "listing",
        json!({"product": "gtin:04006381333931", "url": "https://shop.example/k", "price": {"amount": 1999, "currency": "EUR"}, "availability": "in_stock", "condition": "new"}),
    );
    let l2 = rep(
        &seller,
        4,
        Some(l1.id()),
        "listing",
        l1.id(),
        json!({"product": "gtin:04006381333931", "url": "https://shop.example/k", "price": {"amount": 1899, "currency": "EUR"}, "availability": "in_stock", "condition": "new"}),
    );
    let p3 = rep(&seller, 5, Some(l2.id()), "product", p2.id(), json!({"name": "Plain thing v2", "brand": "Acme"}));
    let f = rec(&att, 1, None, 1, "flag", json!({"target": l1.id().to_hex(), "reason": "spam"}));
    let at = rec(
        &att,
        2,
        Some(f.id()),
        2,
        "attest.identity",
        json!({"subject": seller.author().to_hex(), "person": "11".repeat(32), "level": "business", "expires": 99}),
    );
    let rv = rec(
        &att,
        3,
        Some(at.id()),
        3,
        "review",
        json!({"product": "gtin:04006381333931", "rating": 5, "text": "ok", "listing": l1.id().to_hex()}),
    );
    let rd = rec(
        &att,
        4,
        Some(rv.id()),
        4,
        "redact",
        json!({"target": l1.id().to_hex(), "reason": "illegal", "product": "gtin:04006381333931"}),
    );
    let mut s = MemoryStore::new();
    for r in [&p1, &p2, &l1, &l2, &p3, &f, &at, &rv, &rd] {
        assert_eq!(s.insert(r.clone()), Insert::Inserted);
    }
    assert_eq!(s.by_target(&l1.id()), {
        let mut v = vec![f.id(), rd.id()];
        v.sort();
        v
    });
    let mut g = vec![p1.id(), l1.id(), l2.id(), rv.id(), rd.id()];
    g.sort();
    assert_eq!(s.by_product("gtin:04006381333931"), g);
    let rk = format!("rec:{}", p2.id());
    let mut r = vec![p2.id(), p3.id()];
    r.sort();
    assert_eq!(s.by_product(&rk), r, "replacement keyed by chain root");
    assert_eq!(s.product_keys().len(), 2);
    let mut refs = vec![l2.id(), rv.id()];
    refs.sort();
    assert_eq!(s.referencing(&l1.id()), refs);
    assert_eq!(s.replaced_by(&l1.id()), vec![l2.id()]);
    assert_eq!(s.by_subject(&seller.author()), vec![at.id()]);
    assert_eq!(s.by_person(&att.author(), &perch_core::Hex32([0x11; 32])), vec![at.id()]);
    assert!(s.by_person(&seller.author(), &perch_core::Hex32([0x11; 32])).is_empty());
    assert_eq!(s.by_author_kind(&seller.author(), "product"), vec![p1.id(), p2.id(), p3.id()]);
    assert_eq!(s.by_kind("listing").len(), 2);
    // Removing a body re-indexes: the listing no longer has a product key.
    s.remove_body(&l2.id()).unwrap();
    assert!(!s.by_product("gtin:04006381333931").contains(&l2.id()));
    assert!(s.replaced_by(&l1.id()).contains(&l2.id()), "header index kept");
    // Fork truncation removes index entries.
    let other = rec(&seller, 3, Some(p2.id()), 3, "note", json!({}));
    assert_eq!(s.insert(other), Insert::Fork { pair_changed: true });
    assert!(s.by_target(&l1.id()).contains(&f.id()));
    assert_eq!(s.by_product(&rk), vec![p2.id()]);
    assert!(s.referencing(&l1.id()) == vec![rv.id()]);
    // A header-only product root is keyed rec:<id>.
    let k9 = kp(9);
    let hp = rec(&k9, 1, None, 1, "product", json!({"name": "Hidden", "brand": "B", "gtin": "4006381333931"}));
    s.insert(hp.without_body());
    assert_eq!(s.by_product(&format!("rec:{}", hp.id())), vec![hp.id()]);
    s.insert(hp.clone());
    assert!(s.by_product("gtin:04006381333931").contains(&hp.id()), "body fill re-indexes");
}

#[test]
fn sparse_mode() {
    let k = kp(10);
    let a = k.author();
    let x = chain(&k, 8, 0);
    let y = branch(&k, &x, 6, 1, 1);
    let mut s = MemoryStore::new_sparse();
    assert!(s.is_sparse());
    assert_eq!(s.insert(x[6].clone()), Insert::Inserted);
    assert_eq!(s.insert(x[2].clone()), Insert::Inserted);
    assert_eq!(s.len(), 2);
    assert_eq!(s.head(&a).unwrap().seq, 7);
    // y6 at 6 vs x7 at 7: x7.prev = x6 ≠ y6 → type 2 at 6.
    assert_eq!(s.insert(y[0].clone()), Insert::Fork { pair_changed: true });
    assert_eq!(s.fork_seq(&a), Some(6));
    assert_eq!(s.len(), 1, "records at or above the fork seq dropped");
    assert_eq!(s.insert(x[7].clone()), Insert::AboveFork);
}

#[test]
fn sparse_detects_forks_from_plain_header_only_lines() {
    let k = kp(16);
    let a = k.author();
    let x = chain(&k, 6, 0);
    // Same seq, different ID.
    let y = branch(&k, &x, 4, 2, 1);
    let mut s = MemoryStore::new_sparse();
    let text = format!("{}\n{}\n", x[3].without_body().to_json_string(), y[0].without_body().to_json_string());
    let rep = s.ingest(&text);
    assert_eq!(rep.fork, 1);
    assert_eq!(s.fork_seq(&a), Some(4));
    // Consecutive with prev mismatch, in a fresh store, in reverse order.
    let mut t = MemoryStore::new_sparse();
    let text = format!("{}\n{}\n", y[1].without_body().to_json_string(), x[3].without_body().to_json_string());
    t.ingest(&text);
    assert_eq!(t.fork_seq(&a), Some(4));
    assert_eq!(t.evidence(&a).unwrap().ids(), [x[3].id(), y[1].id()], "sorted by (seq, id)");
}

#[test]
fn hold_buffer_is_bounded() {
    let k = kp(11);
    let c = chain(&k, 40, 0);
    let mut s = MemoryStore::new();
    s.set_hold_limits(HoldLimits { max_records: 10, max_bytes: 1 << 20, per_author: 5 });
    for r in &c[1..] {
        s.insert(r.clone());
    }
    assert_eq!(s.held_count(), 5);
    s.insert(c[0].clone());
    assert_eq!(s.head(&k.author()).unwrap().seq, 6);
}

#[test]
fn admission_limits() {

    let k = kp(12);
    let c = chain(&k, 5, 0);
    let mut s = MemoryStore::new();
    s.set_admission(Some(std::sync::Arc::new(|_: &MemoryStore, _: &AuthorId| AdmissionLimit {
        records: 3,
        bytes: u64::MAX,
        pool_rank: None,
    })));
    let outs: Vec<Insert> = c.iter().map(|r| s.insert(r.clone())).collect();
    assert_eq!(outs[2], Insert::Inserted);
    assert!(matches!(outs[3], Insert::Rejected(Reject::Admission(_))));
    assert_eq!(s.head(&k.author()).unwrap().seq, 3);
}

#[test]
fn pool_budget_keeps_lowest_ranks_as_whole_logs() {
    // Every author is pooled; rank = author bytes, so the order is known.
    let keys: Vec<Keypair> = (30..42).map(kp).collect();
    let mut by_rank: Vec<&Keypair> = keys.iter().collect();
    by_rank.sort_by_key(|k| k.author());
    let ranked: Vec<AuthorId> = by_rank.iter().map(|k| k.author()).collect();
    let log_size = |k: &Keypair| chain(k, 2, 0).iter().map(wire_len_of).sum::<u64>();
    let five: u64 = by_rank[..5].iter().map(|k| log_size(k)).sum();
    let mut s = MemoryStore::new();
    s.set_admission(Some(std::sync::Arc::new(|_: &MemoryStore, a: &AuthorId| AdmissionLimit {
        records: 2,
        bytes: u64::MAX,
        pool_rank: Some(a.0),
    })));
    s.set_pool_budget(five + 10);
    // Arrival order is not rank order.
    for k in keys.iter().rev() {
        for r in chain(k, 3, 0) {
            s.insert(r);
        }
    }
    assert_eq!(s.authors(), ranked[..5].to_vec(), "the five lowest ranks are kept");
    for a in &ranked[..5] {
        assert_eq!(s.head(a).unwrap().seq, 2, "whole prefixes within the per-log limit");
    }
    assert!(s.pool_bytes() <= five + 10);
    assert!(s.needs_compaction());
    // A higher-ranked author is refused while the pool is full.
    let o = s.insert(chain(by_rank[9], 1, 0)[0].clone());
    assert!(matches!(o, Insert::Rejected(Reject::Admission(_))), "{o:?}");
    // Shrinking the budget drops the highest ranks on rebuild.
    let three: u64 = by_rank[..3].iter().map(|k| log_size(k)).sum();
    s.set_pool_budget(three + 10);
    let dropped = s.rebuild_pool(false);
    assert_eq!(dropped, vec![ranked[4], ranked[3]]);
    assert_eq!(s.authors(), ranked[..3].to_vec());
    // Authors that leave the pool stop counting against it.
    s.set_admission(Some(std::sync::Arc::new(|_: &MemoryStore, _: &AuthorId| AdmissionLimit::UNLIMITED)));
    s.rebuild_pool(true);
    assert_eq!(s.pool_len(), 0);
    assert_eq!(s.pool_bytes(), 0);
}

fn wire_len_of(r: &Record) -> u64 {
    // Same approximation as the store's limits.
    (r.header_canonical().len() + 160 + r.body_canonical().map_or(0, |b| b.len() + 8)) as u64
}

#[test]
fn export_replay_rebuilds_state_including_ts_violating_evidence() {
    let k = kp(13);
    let x = chain(&k, 4, 0);
    // Fork pair whose lower record has ts below its predecessor: only recognizable as a pair.
    let bad = rec(&k, 5, Some(x[3].id()), 1, "note", json!({"bad": 1}));
    let other = rec(&k, 6, Some(RecordId([7; 32])), 200, "note", json!({}));
    let mut s = MemoryStore::new();
    for r in &x {
        s.insert(r.clone());
    }
    assert_eq!(s.insert_evidence(bad.clone(), other.clone()), Insert::Fork { pair_changed: true });
    let k2 = kp(14);
    for r in chain(&k2, 3, 0) {
        s.insert(r);
    }
    let text = s.export_jsonl();
    let mut t = MemoryStore::new();
    let rep = t.ingest(&text);
    assert_eq!(rep.errors.len(), 1, "only the ts-violating record is refused on its own: {:?}", rep.errors);
    assert_eq!(state(&s), state(&t));
    assert_eq!(t.fork_seq(&k.author()), Some(5));
}

#[test]
fn ingest_json_array_and_errors() {
    let k = kp(15);
    let c = chain(&k, 2, 0);
    let arr = format!("[{},{}, {{\"v\":1}}]", c[0].to_json_string(), c[1].to_json_string());
    let mut s = MemoryStore::new();
    let rep = s.ingest(&arr);
    assert_eq!(rep.inserted, 2);
    assert_eq!(rep.rejected, 1);
    let rep = s.ingest(&format!("{}\nnot json\n", c[0].to_json_string()));
    assert_eq!(rep.duplicate, 1);
    assert_eq!(rep.errors.len(), 1);
    assert_eq!(rep.errors[0].0, 2);
}

#[cfg(not(target_arch = "wasm32"))]
mod persistence {
    use super::*;
    use std::io::Write;

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("perch-store-test-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn round_trip_partial_line_and_compaction() {
        let dir = tmpdir("rt");
        let path = dir.join("records.jsonl");
        let k = kp(20);
        let x = chain(&k, 5, 0);
        let y = branch(&k, &x, 4, 1, 1);
        let secret = rec(&kp(21), 1, None, 1, "note", json!({"text": "personal data"}));
        let before;
        {
            let (mut s, rep) = MemoryStore::open(&path, Mode::Full, []).unwrap();
            assert_eq!(rep.lines, 0);
            for r in &x {
                s.insert(r.clone());
            }
            s.insert(y[0].clone());
            s.insert(secret.clone());
            s.insert(x[0].without_body());
            s.flush().unwrap();
            before = state(&s);
        }
        // Simulate a crash in the middle of a write.
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            f.write_all(b"{\"author\":\"ab").unwrap();
        }
        {
            let (mut s, rep) = MemoryStore::open(&path, Mode::Full, []).unwrap();
            assert!(rep.truncated_bytes > 0);
            assert!(rep.invalid.is_empty());
            assert_eq!(state(&s), before);
            assert!(s.get(&secret.id()).unwrap().has_body());
            assert!(s.remove_body(&secret.id()).unwrap());
            s.flush().unwrap();
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("personal data"), "compaction removed the body from disk");
        {
            let (s, _) = MemoryStore::open(&path, Mode::Full, [secret.id()]).unwrap();
            assert_eq!(state(&s), before);
            assert!(!s.get(&secret.id()).unwrap().has_body());
        }
        // Opening with a removal list for a body still on disk compacts it away.
        let path2 = dir.join("two.jsonl");
        {
            let (mut s, _) = MemoryStore::open(&path2, Mode::Full, []).unwrap();
            s.insert(secret.clone());
            s.flush().unwrap();
        }
        let (_s, rep) = MemoryStore::open(&path2, Mode::Full, [secret.id()]).unwrap();
        assert!(rep.compacted);
        assert!(!std::fs::read_to_string(&path2).unwrap().contains("personal data"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

mod fork_order {
    use super::*;
    use proptest::prelude::*;

    /// Lowest pair over all records, by brute force.
    fn lowest_pair(all: &[Record]) -> Option<EvidencePair> {
        let mut best: Option<EvidencePair> = None;
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                if let Ok(p) = EvidencePair::new(all[i].clone(), all[j].clone()) {
                    best = Some(match best {
                        Some(b) => b.lowest(p),
                        None => p,
                    });
                }
            }
        }
        best
    }

    fn shuffled(items: &[Record], order: &[usize], dup: &[bool], strip: &[bool]) -> Vec<Record> {
        let mut out = Vec::new();
        for (i, &o) in order.iter().enumerate() {
            let r = &items[o % items.len()];
            let r = if strip[i % strip.len()] { r.without_body() } else { r.clone() };
            out.push(r.clone());
            if dup[i % dup.len()] {
                out.push(items[o % items.len()].clone());
            }
        }
        // Make sure every record arrives with its body at least once.
        out.extend(items.iter().cloned());
        out
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        /// Two histories of one author, delivered in any order, with
        /// duplicates and header-only copies, give the same prefix and the
        /// brute-force lowest pair.
        #[test]
        fn two_branch_fork_converges(
            len in 1u64..8, from_off in 0u64..8, blen in 1u64..6,
            o1 in proptest::collection::vec(0usize..64, 1..40),
            o2 in proptest::collection::vec(0usize..64, 1..40),
            dup in proptest::collection::vec(any::<bool>(), 1..8),
            strip in proptest::collection::vec(any::<bool>(), 1..8),
            seed in 1u8..200,
        ) {
            let k = kp(seed);
            let x = chain(&k, len, 0);
            let from = 1 + from_off % len;
            let y = branch(&k, &x, from, blen, 1);
            let all: Vec<Record> = x.iter().chain(y.iter()).cloned().collect();
            let mut s1 = MemoryStore::new();
            let mut s2 = MemoryStore::new();
            for r in shuffled(&all, &o1, &dup, &strip) { s1.insert(r); }
            for r in shuffled(&all, &o2, &strip, &dup) { s2.insert(r); }
            prop_assert_eq!(state(&s1), state(&s2));
            let want = lowest_pair(&all).map(|p| p.ids());
            prop_assert_eq!(s1.evidence(&k.author()).map(|p| p.ids()), want);
            let n = want.map(|_| from).unwrap_or(u64::MAX);
            let expect_seq = (n - 1).min(len.max(from + blen - 1));
            prop_assert_eq!(s1.head(&k.author()).unwrap().seq, expect_seq);
            for r in s1.log(&k.author(), 1, usize::MAX) { prop_assert!(r.has_body()); }
        }
    }
}
