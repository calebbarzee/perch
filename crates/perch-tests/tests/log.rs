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

#[test]
fn evidence_pair_replaced_by_lower_pair() {
    require_sut!(Store);
    // Fork at seq 3 first, then evidence at seq 2 arrives: store must move to seq 2.
    let l = three("log-lower");
    let alt3 = l.fork_at(3, "profile", json!({ "name": "X3", "role": "buyer" }));
    let alt2 = l.fork_at(2, "profile", json!({ "name": "X2", "role": "buyer" }));
    let mut s = Store::new();
    for r in &l.records {
        s.ingest(&r.wire());
    }
    s.ingest(&alt3.wire());
    assert_eq!(s.fork_seq(&l.author()), Some(3));
    assert_eq!(s.head_seq(&l.author()), 2);
    s.ingest(&alt2.wire());
    assert_eq!(s.fork_seq(&l.author()), Some(2), "lower fork seq wins");
    assert_eq!(s.head_seq(&l.author()), 1);
}

#[test]
fn export_replay_reproduces_state() {
    require_sut!(Store);
    let l = three("log-replay-export");
    let alt = l.fork_at(2, "profile", json!({ "name": "X", "role": "buyer" }));
    let mut other = Log::new("log-replay-other");
    other.profile("O", "buyer");
    let mut s = Store::new();
    for r in l.records.iter().chain(other.records.iter()) {
        s.ingest(&r.wire());
    }
    s.ingest(&alt.wire());
    let mut t = Store::new();
    for line in s.export().lines() {
        t.ingest(line);
    }
    assert_eq!(s.export(), t.export());
    assert_eq!(t.fork_seq(&l.author()), Some(2));
    assert_eq!(s.evidence(&l.author()), t.evidence(&l.author()));
}

#[test]
fn many_forks_keep_one_pair() {
    require_sut!(Store);
    // 50 conflicting records at seq 2: one evidence pair, the lowest by (seq, id).
    let l = three("log-many");
    let alts: Vec<_> = (0..50).map(|i| l.fork_at(2, "profile", json!({ "name": format!("X{i}"), "role": "buyer" }))).collect();
    let mut s = Store::new();
    for r in &l.records {
        s.ingest(&r.wire());
    }
    for a in &alts {
        s.ingest(&a.wire());
    }
    let mut ids: Vec<String> = alts.iter().map(|a| a.id.clone()).chain([l.records[1].id.clone()]).collect();
    ids.sort();
    assert_eq!(s.evidence(&l.author()), Some((ids[0].clone(), ids[1].clone())));
    let lines = s.export().lines().filter(|x| x.contains(&l.author())).count();
    assert_eq!(lines, 3, "prefix (1) + evidence pair (2), nothing else kept");
}

#[test]
fn sparse_store_accepts_records_without_prefix() {
    require_sut!(Store);
    let l = three("log-sparse");
    let mut s = Store::new_sparse();
    assert_eq!(s.ingest(&l.records[2].wire()), Ingest::Inserted);
    assert!(s.has(&l.records[2].id));
    let bad = l.records[0].wire().replace(&l.records[0].sig, &"0".repeat(128));
    assert!(matches!(s.ingest(&bad), Ingest::Rejected(_)), "sparse stores still verify signatures");
}

/// Deliver `all` in every order (twice each, to drain gaps) and return the set
/// of distinct final states.
fn states(author: &str, all: &[String]) -> std::collections::BTreeSet<String> {
    let idx: Vec<usize> = (0..all.len()).collect();
    let mut out = std::collections::BTreeSet::new();
    for p in permutations(&idx) {
        let mut s = Store::new();
        for _ in 0..2 {
            for &i in &p {
                s.ingest(&all[i]);
            }
        }
        out.insert(format!("{:?}|{}|{:?}|{}", s.fork_seq(author), s.head_seq(author), s.evidence(author), s.export()));
    }
    out
}

#[test]
fn branching_fork_state_independent_of_arrival_order() {
    require_sut!(Store);
    // Honest 1-2-3 and a branch 2'-3' whose 3' links to 2'. Type-1 and type-2
    // evidence both exist; every order must reach the same pair.
    let l = three("log-branch");
    let mut branch = l.clone();
    branch.records.truncate(1);
    let b2 = branch.add_with("profile", json!({ "name": "B2", "role": "buyer" }), Opts { ts: Some(T0 + 1500), ..Default::default() });
    let b3 = branch.add_with("profile", json!({ "name": "B3", "role": "buyer" }), Opts { ts: Some(T0 + 2500), ..Default::default() });
    let all = vec![l.records[0].wire(), l.records[1].wire(), l.records[2].wire(), b2.wire(), b3.wire()];
    let st = states(&l.author(), &all);
    assert_eq!(st.len(), 1, "stores disagree: {st:#?}");
    assert!(st.iter().next().unwrap().starts_with("Some(2)|1|"));
}

#[test]
fn ts_decreasing_conflict_state_independent_of_arrival_order() {
    require_sut!(Store);
    // The conflicting seq-2 record has ts below seq 1. Arriving before the honest
    // seq 2 it is a ts violation; arriving after it, it is a same-seq conflict.
    // Both orders must end in the same state (contract 5.2: "regardless of arrival order").
    let mut l = Log::new("log-tsfork");
    l.add_with("profile", json!({ "name": "A", "role": "buyer" }), Opts { ts: Some(T0 + 5000), ..Default::default() });
    l.add_with("profile", json!({ "name": "B", "role": "buyer" }), Opts { ts: Some(T0 + 6000), ..Default::default() });
    let alt = l.add_with(
        "profile",
        json!({ "name": "Old", "role": "buyer" }),
        Opts { seq: Some(2), ts: Some(T0 + 1000), ..Default::default() },
    );
    let all = vec![l.records[0].wire(), l.records[1].wire(), alt.wire()];
    let st = states(&l.author(), &all);
    assert_eq!(st.len(), 1, "arrival order changes the state: {st:#?}");
}
