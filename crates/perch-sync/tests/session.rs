use perch_core::{Keypair, Record, RecordId, Unsigned, sign_record};
use perch_store::{MemoryStore, Store};
use perch_sync::message::{ErrorCode, HeadEntry, Limits, Message, Role, WantEntry};
use perch_sync::{Session, SessionConfig, State};
use serde_json::json;

fn cfg(n: u8) -> SessionConfig {
    SessionConfig::new(format!("{:032x}", n), Role::Node)
}

fn chain(seed: u8, n: u64) -> Vec<Record> {
    let k = Keypair::from_seed([seed; 32]);
    let mut out: Vec<Record> = Vec::new();
    for s in 1..=n {
        let body = json!({"n": s, "text": "x".repeat(100)}).as_object().unwrap().clone();
        let prev = out.last().map(|r| r.id());
        out.push(sign_record(&k, Unsigned { seq: s, prev, ts: s, kind: "note".into(), replaces: None, body }).unwrap());
    }
    out
}

fn store_with(records: &[Record]) -> MemoryStore {
    let mut s = MemoryStore::new();
    s.set_track_changes(true);
    for r in records {
        s.insert(r.clone());
    }
    s.drain_changes();
    s
}

fn hello_text(node: u8, limits: Limits) -> String {
    let mut c = cfg(node);
    c.limits = limits;
    let mut s = Session::new(99, c, true);
    s.start()[0].to_json()
}

/// A session past the handshake, with the peer's get_heads already answered.
fn live_session(store: &mut MemoryStore, limits: Limits) -> Session {
    let mut s = Session::new(1, cfg(1), false);
    s.start();
    let out = s.on_message(store, &hello_text(2, limits)).unwrap();
    assert!(matches!(out[0], Message::GetHeads { .. }));
    let out = s.on_message(store, &Message::Heads { heads: vec![], next: None }.to_json()).unwrap();
    assert_eq!(s.state(), State::Live);
    assert!(out.iter().any(|m| matches!(m, Message::GetPeers)));
    s
}

fn text(m: Message) -> String {
    m.to_json()
}

#[test]
fn handshake_rules() {
    let mut store = MemoryStore::new();
    let mut s = Session::new(1, cfg(1), true);
    assert!(matches!(s.start()[0], Message::Hello(_)));
    let e = s.on_message(&mut store, &text(Message::GetPeers)).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadMessage);
    assert!(s.is_closed());

    let mut s = Session::new(1, cfg(1), true);
    let e = s.on_message(&mut store, &hello_text(1, Limits::default())).unwrap_err();
    assert_eq!(e.code, ErrorCode::SelfConnection);

    let mut s = Session::new(1, cfg(1), true);
    let v2 = hello_text(2, Limits::default()).replace("\"versions\":[1]", "\"versions\":[2,3]");
    assert_eq!(s.on_message(&mut store, &v2).unwrap_err().code, ErrorCode::UnsupportedVersion);

    let mut s = Session::new(1, cfg(1), true);
    s.on_message(&mut store, &hello_text(2, Limits::default())).unwrap();
    assert_eq!(s.state(), State::Syncing);
    assert_eq!(s.on_message(&mut store, &hello_text(2, Limits::default())).unwrap_err().code, ErrorCode::BadMessage);
}

#[test]
fn heads_are_paged() {
    let mut recs = Vec::new();
    for a in 0..10u8 {
        recs.extend(chain(a + 10, 2));
    }
    let mut store = store_with(&recs);
    let mut s = live_session(&mut store, Limits::default());
    let mut after = None;
    let mut seen = Vec::new();
    loop {
        let out = s.on_message(&mut store, &text(Message::GetHeads { after: after.clone(), limit: 3 })).unwrap();
        let Message::Heads { heads, next } = &out[0] else { panic!("{out:?}") };
        assert!(heads.len() <= 3);
        seen.extend(heads.iter().map(|h| h.author.clone()));
        match next {
            Some(n) => after = Some(n.clone()),
            None => break,
        }
    }
    assert_eq!(seen.len(), 10);
    let mut sorted = seen.clone();
    sorted.sort();
    assert_eq!(seen, sorted);
}

#[test]
fn want_answers_in_order_with_truncation() {
    let c = chain(30, 20);
    let mut store = store_with(&c);
    let limits = Limits { max_records: 7, ..Limits::default() };
    let mut s = live_session(&mut store, limits);
    let author = c[0].author().to_hex();
    let out = s
        .on_message(&mut store, &text(Message::Want { wants: vec![WantEntry { author: author.clone(), from_seq: 3, to_seq: None }] }))
        .unwrap();
    let Message::Records { records, truncated } = &out[0] else { panic!() };
    assert_eq!(records.len(), 7);
    assert!(truncated);
    let seqs: Vec<u64> = records.iter().map(|r| Record::from_json_str(r).unwrap().seq()).collect();
    assert_eq!(seqs, (3..10).collect::<Vec<_>>());
    let out = s
        .on_message(&mut store, &text(Message::Want { wants: vec![WantEntry { author, from_seq: 18, to_seq: Some(19) }] }))
        .unwrap();
    let Message::Records { records, truncated } = &out[0] else { panic!() };
    assert_eq!(records.len(), 2);
    assert!(!truncated);
}

#[test]
fn heads_trigger_wants_and_records_are_applied() {
    let c = chain(31, 5);
    let mut store = store_with(&c[..2]);
    let mut s = Session::new(1, cfg(1), true);
    s.start();
    s.on_message(&mut store, &hello_text(2, Limits::default())).unwrap();
    let heads = vec![HeadEntry { author: c[0].author().to_hex(), seq: 5, id: Some(c[4].id().to_hex()), forked: false, evidence: None }];
    let out = s.on_message(&mut store, &text(Message::Heads { heads, next: None })).unwrap();
    let want = out.iter().find_map(|m| if let Message::Want { wants } = m { Some(wants.clone()) } else { None }).unwrap();
    assert_eq!(want[0].from_seq, 3);
    let records: Vec<String> = c[2..].iter().map(|r| r.to_json_string()).collect();
    s.on_message(&mut store, &text(Message::Records { records, truncated: false })).unwrap();
    assert_eq!(store.head(&c[0].author()).unwrap().seq, 5);
    assert_eq!(s.stats.records_accepted, 3);
    let changes = store.drain_changes();
    assert_eq!(changes.len(), 3);
    // The peer already has them: no echo.
    assert!(s.on_changes(&store, &changes, Some(1)).is_empty());
}

#[test]
fn scoring_and_unsolicited_responses() {
    let mut store = MemoryStore::new();
    let mut s = live_session(&mut store, Limits::default());
    s.on_message(&mut store, &text(Message::Records { records: vec![], truncated: false })).unwrap();
    assert_eq!(s.score(), 10);
    s.on_message(&mut store, r#"{"type":"something_new"}"#).unwrap();
    assert_eq!(s.score(), 11);
    let bad = chain(32, 1)[0].to_json_string().replace("\"seq\":1", "\"seq\":2");
    s.on_message(&mut store, &text(Message::Push { records: vec![bad.clone()], evidence: vec![] })).unwrap();
    assert_eq!(s.score(), 61);
    let e = s.on_message(&mut store, &text(Message::Push { records: vec![bad], evidence: vec![] })).unwrap_err();
    assert_eq!(e.code, ErrorCode::Misbehavior);
    assert!(s.is_closed());
    assert!(store.is_empty());
}

#[test]
fn oversize_message_is_fatal() {
    let mut store = MemoryStore::new();
    let mut c = cfg(1);
    c.limits.max_message_bytes = 300_000;
    let mut s = Session::new(1, c, true);
    s.on_message(&mut store, &hello_text(2, Limits::default())).unwrap();
    let big = format!(r#"{{"type":"push","records":[],"pad":"{}"}}"#, "x".repeat(300_001));
    assert_eq!(s.on_message(&mut store, &big).unwrap_err().code, ErrorCode::TooLarge);
}

#[test]
fn bodies_and_evidence_requests() {
    let c = chain(33, 3);
    let k = Keypair::from_seed([33; 32]);
    let body = json!({"other": true}).as_object().unwrap().clone();
    let alt = sign_record(&k, Unsigned { seq: 3, prev: Some(c[1].id()), ts: 3, kind: "note".into(), replaces: None, body }).unwrap();
    let mut store = store_with(&c);
    store.insert(alt.clone());
    let author = c[0].author();
    assert_eq!(store.fork_seq(&author), Some(3));
    let mut s = live_session(&mut store, Limits::default());
    let out = s.on_message(&mut store, &text(Message::WantEvidence { authors: vec![author.to_hex()] })).unwrap();
    let Message::Evidence { pairs } = &out[0] else { panic!() };
    assert_eq!(pairs.len(), 1);
    let ids: Vec<RecordId> = pairs[0].iter().map(|r| Record::from_json_str(r).unwrap().id()).collect();
    assert_eq!(ids, store.evidence(&author).unwrap().ids().to_vec());
    let want = vec![c[0].id().to_hex(), c[2].id().to_hex()];
    let out = s.on_message(&mut store, &text(Message::WantBodies { ids: want })).unwrap();
    let Message::Bodies { records, .. } = &out[0] else { panic!() };
    assert_eq!(records.len(), 1, "evidence records have no stored body");
    assert!(Record::from_json_str(&records[0]).unwrap().has_body());
}

#[test]
fn false_fork_hint_costs_points() {
    let mut store = MemoryStore::new();
    let mut s = Session::new(1, cfg(1), true);
    s.on_message(&mut store, &hello_text(2, Limits::default())).unwrap();
    let a = "ab".repeat(32);
    let heads = vec![HeadEntry { author: a, seq: 0, id: None, forked: true, evidence: Some(["01".repeat(32), "02".repeat(32)]) }];
    let out = s.on_message(&mut store, &text(Message::Heads { heads, next: None })).unwrap();
    assert!(out.iter().any(|m| matches!(m, Message::WantEvidence { .. })));
    s.on_message(&mut store, &text(Message::Evidence { pairs: vec![] })).unwrap();
    assert_eq!(s.score(), 20);
}

#[test]
fn peers_exchange() {
    let mut store = MemoryStore::new();
    let mut s = live_session(&mut store, Limits::default());
    s.set_shareable_addresses(vec!["wss://a.example.org/v1/sync".into()]);
    let out = s.on_message(&mut store, &text(Message::GetPeers)).unwrap();
    assert_eq!(out[0], Message::Peers { peers: vec!["wss://a.example.org/v1/sync".into()] });
    s.on_message(&mut store, &text(Message::Peers { peers: vec!["wss://b.example.org/v1/sync".into(), "http://no".into()] }))
        .unwrap();
    assert_eq!(s.take_learned_addresses(), vec!["wss://b.example.org/v1/sync".to_string()]);
    // A second unsolicited answer costs points.
    s.on_message(&mut store, &text(Message::Peers { peers: vec![] })).unwrap();
    assert_eq!(s.score(), 10);
}
