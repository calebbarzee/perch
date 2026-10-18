//! Log level (contract 4.4, 5.1, 5.2): continuity, forks, replays.

use perch_tests::fixture::{Log, Opts, T0};
use perch_tests::require_sut;
use perch_tests::sut::{Ingest, Store};
use serde_json::json;

fn three(label: &str) -> Log {
    let mut l = Log::new(label);
    l.profile("A", "buyer");
    l.profile("B", "buyer");
    l.profile("C", "buyer");
    l
}

#[test]
fn in_order_log_inserted() {
    require_sut!(Store);
    let l = three("log-inorder");
    let mut s = Store::new();
    for r in &l.records {
        assert_eq!(s.ingest(&r.wire()), Ingest::Inserted);
    }
    assert_eq!(s.head_seq(&l.author()), 3);
}

#[test]
fn replayed_record_is_duplicate() {
    require_sut!(Store);
    let l = three("log-replay");
    let mut s = Store::new();
    s.ingest_all(&l.records.iter().map(|r| r.wire()).collect::<Vec<_>>());
    for r in &l.records {
        assert_eq!(s.ingest(&r.wire()), Ingest::Duplicate);
    }
    assert_eq!(s.export().lines().count(), 3, "replays must not add lines");
}

#[test]
fn gap_is_held_not_stored() {
    require_sut!(Store);
    let l = three("log-gap");
    let mut s = Store::new();
    assert_eq!(s.ingest(&l.records[0].wire()), Ingest::Inserted);
    assert_eq!(s.ingest(&l.records[2].wire()), Ingest::Gap);
    assert_eq!(s.head_seq(&l.author()), 1, "complete-prefix rule");
    assert!(!s.export().contains(&l.records[2].id));
}

#[test]
fn same_seq_fork_detected_and_truncated() {
    require_sut!(Store);
    let l = three("log-fork");
    let alt = l.fork_at(2, "profile", json!({ "name": "Other", "role": "buyer" }));
    let mut s = Store::new();
    s.ingest_all(&l.records.iter().map(|r| r.wire()).collect::<Vec<_>>());
    assert_eq!(s.ingest(&alt.wire()), Ingest::Fork);
    assert_eq!(s.fork_seq(&l.author()), Some(2));
    assert_eq!(s.head_seq(&l.author()), 1, "truncated to fork seq - 1");
    let ev = s.evidence(&l.author()).expect("evidence pair kept");
    let mut ids = [l.records[1].id.clone(), alt.id.clone()];
    ids.sort();
    assert_eq!((ev.0, ev.1), (ids[0].clone(), ids[1].clone()), "pair sorted by (seq, id)");
}

#[test]
fn prev_mismatch_fork_detected() {
    require_sut!(Store);
    // Record at seq 3 whose prev is not the stored seq 2: fork at seq 2.
    let l = three("log-prev");
    let alt2 = l.fork_at(2, "profile", json!({ "name": "Alt", "role": "buyer" }));
    let mut other = l.clone();
    other.records.truncate(1);
    other.records.push(alt2.clone());
    let alt3 = other.add("profile", json!({ "name": "Alt3", "role": "buyer" }));
    let mut s = Store::new();
    s.ingest(&l.records[0].wire());
    s.ingest(&l.records[1].wire());
    let out = s.ingest(&alt3.wire());
    assert_eq!(out, Ingest::Fork, "seq 3 with prev != stored seq 2 is fork evidence");
    assert_eq!(s.fork_seq(&l.author()), Some(2));
    assert_eq!(s.head_seq(&l.author()), 1);
}

#[test]
fn fork_state_independent_of_arrival_order() {
    require_sut!(Store);
    let l = three("log-order");
    let alt2 = l.fork_at(2, "profile", json!({ "name": "X2", "role": "buyer" }));
    let alt3 = l.fork_at(3, "profile", json!({ "name": "X3", "role": "buyer" }));
    let all = vec![
        l.records[0].wire(),
        l.records[1].wire(),
        l.records[2].wire(),
        alt2.wire(),
        alt3.wire(),
    ];
    // Every permutation of the 5 records gives the same fork seq, head, evidence, export.
    let mut results = std::collections::BTreeSet::new();
    let idx: Vec<usize> = (0..all.len()).collect();
    for p in permutations(&idx) {
        let mut s = Store::new();
        // Deliver twice so held gaps get resolved regardless of order.
        for _ in 0..2 {
            for &i in &p {
                s.ingest(&all[i]);
            }
        }
        results.insert(format!(
            "{:?}|{}|{:?}|{}",
            s.fork_seq(&l.author()),
            s.head_seq(&l.author()),
            s.evidence(&l.author()),
            s.export()
        ));
    }
    assert_eq!(results.len(), 1, "stores disagree depending on arrival order: {results:#?}");
    assert!(results.iter().next().unwrap().starts_with("Some(2)|1|"));
}

fn permutations(v: &[usize]) -> Vec<Vec<usize>> {
    if v.len() <= 1 {
        return vec![v.to_vec()];
    }
    let mut out = vec![];
    for i in 0..v.len() {
        let mut rest = v.to_vec();
        let x = rest.remove(i);
        for mut p in permutations(&rest) {
            p.insert(0, x);
            out.push(p);
        }
    }
    out
}

#[test]
fn records_after_fork_not_accepted() {
    require_sut!(Store);
    let mut l = three("log-after");
    let alt = l.fork_at(2, "profile", json!({ "name": "X", "role": "buyer" }));
    let r4 = l.profile("D", "buyer");
    let mut s = Store::new();
    s.ingest_all(&l.records[..3].iter().map(|r| r.wire()).collect::<Vec<_>>());
    s.ingest(&alt.wire());
    s.ingest(&r4.wire());
    assert_eq!(s.head_seq(&l.author()), 1);
    assert!(!s.has(&r4.id));
}

#[test]
fn ts_decrease_rejected() {
    require_sut!(Store);
    let mut l = Log::new("log-ts");
    l.add_with("profile", json!({ "name": "A", "role": "buyer" }), Opts { ts: Some(T0 + 5000), ..Default::default() });
    let r2 = l.add_with("profile", json!({ "name": "B", "role": "buyer" }), Opts { ts: Some(T0 + 4999), ..Default::default() });
    let mut s = Store::new();
    s.ingest(&l.records[0].wire());
    assert!(matches!(s.ingest(&r2.wire()), Ingest::Rejected(_)), "ts below previous record must be rejected");
    assert_eq!(s.head_seq(&l.author()), 1);
}

#[test]
fn equal_ts_accepted() {
    require_sut!(Store);
    let mut l = Log::new("log-ts-eq");
    l.add_with("profile", json!({ "name": "A", "role": "buyer" }), Opts { ts: Some(T0), ..Default::default() });
    l.add_with("profile", json!({ "name": "B", "role": "buyer" }), Opts { ts: Some(T0), ..Default::default() });
    let mut s = Store::new();
    for r in &l.records {
        assert_eq!(s.ingest(&r.wire()), Ingest::Inserted);
    }
}

#[test]
fn invalid_record_never_stored() {
    require_sut!(Store);
    let l = three("log-invalid");
    let bad = l.records[0].wire().replace(&l.records[0].sig, &"0".repeat(128));
    let mut s = Store::new();
    assert!(matches!(s.ingest(&bad), Ingest::Rejected(_)));
    assert_eq!(s.head_seq(&l.author()), 0);
    assert!(s.export().is_empty());
}
