//! Audit #6: a peer cannot grow session state without bound by answering
//! every `get_heads` with a new page. Adapted from the audit probe.

use perch_store::MemoryStore;
use perch_sync::message::ErrorCode;
use perch_sync::{Message, Role, Session, SessionConfig};

fn hello() -> String {
    format!(
        r#"{{"type":"hello","protocol":"perch-sync","versions":[1],"node":"{}","role":"node","software":"evil"}}"#,
        "b".repeat(32)
    )
}

#[test]
fn endless_heads_paging_is_closed() {
    let mut store = MemoryStore::new();
    let mut cfg = SessionConfig::new("a".repeat(32), Role::Node);
    cfg.max_extra_authors = 20_000;
    let mut s = Session::new(1, cfg, false);
    let _ = s.start();
    let mut out = s.on_message(&mut store, &hello()).unwrap();
    let mut last;
    let mut closed_at = None;
    for round in 0..200u64 {
        assert!(out.iter().any(|m| matches!(m, Message::GetHeads { .. })), "round {round}");
        // Fresh, ascending authors after the cursor, so the page itself is well formed.
        let authors: Vec<String> = (0..4096u64).map(|i| format!("{:064x}", round * 4096 + i + 1)).collect();
        let entries: Vec<String> =
            authors.iter().map(|a| format!(r#"{{"author":"{a}","seq":5,"id":"{}"}}"#, "c".repeat(64))).collect();
        last = authors.last().unwrap().clone();
        let msg = format!(r#"{{"type":"heads","heads":[{}],"next":"{last}"}}"#, entries.join(","));
        match s.on_message(&mut store, &msg) {
            Ok(o) => out = o,
            Err(e) => {
                assert_eq!(e.code, ErrorCode::Misbehavior);
                closed_at = Some(round);
                break;
            }
        }
    }
    assert!(s.is_closed(), "session must close");
    assert!(closed_at.is_some_and(|r| r < 10), "closed after {closed_at:?} pages");
    let (_, ahead) = s.behind_ahead(&store);
    assert!(ahead <= 24_096, "peer state stays bounded: {ahead}");
}

#[test]
fn heads_pages_must_advance() {
    let mut store = MemoryStore::new();
    let mut s = Session::new(1, SessionConfig::new("a".repeat(32), Role::Node), false);
    let _ = s.start();
    s.on_message(&mut store, &hello()).unwrap();
    let a = "5".repeat(64);
    let page = format!(r#"{{"type":"heads","heads":[{{"author":"{a}","seq":1,"id":"{}"}}],"next":"{a}"}}"#, "c".repeat(64));
    s.on_message(&mut store, &page).unwrap();
    // Repeating the same page after cursor `a` is refused.
    let e = s.on_message(&mut store, &page).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadMessage);
    // An empty page with `next` is refused.
    let mut s = Session::new(1, SessionConfig::new("a".repeat(32), Role::Node), false);
    let _ = s.start();
    s.on_message(&mut store, &hello()).unwrap();
    let e = s.on_message(&mut store, &format!(r#"{{"type":"heads","heads":[],"next":"{a}"}}"#)).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadMessage);
}

fn hex(n: u64, salt: u64) -> String {
    perch_core::ids::encode_hex(&perch_core::sha256(&[n.to_le_bytes(), salt.to_le_bytes()].concat()))
}

/// Re-audit probe: state must not accumulate across refresh rounds.
#[test]
fn peer_state_is_bounded_across_refresh_rounds() {
    let mut store = MemoryStore::new();
    let mut s = Session::new(1, SessionConfig::new("a".repeat(32), Role::Node), false);
    let _ = s.start();
    let mut out = s.on_message(&mut store, &hello()).unwrap();
    for round in 0..5u64 {
        let mut authors: Vec<String> = (0..24 * 4096).map(|i| hex(i, round)).collect();
        authors.sort();
        let pages: Vec<&[String]> = authors.chunks(4096).collect();
        for (i, page) in pages.iter().enumerate() {
            assert!(out.iter().any(|m| matches!(m, Message::GetHeads { .. })), "round {round} page {i}");
            let entries: Vec<String> =
                page.iter().map(|a| format!(r#"{{"author":"{a}","seq":5,"id":"{}"}}"#, "c".repeat(64))).collect();
            let next = if i + 1 < pages.len() { format!(r#","next":"{}""#, page.last().unwrap()) } else { String::new() };
            let msg = format!(r#"{{"type":"heads","heads":[{}]{next}}}"#, entries.join(","));
            out = s.on_message(&mut store, &msg).unwrap_or_else(|e| panic!("round {round} page {i}: {e}"));
        }
        let (_, ahead) = s.behind_ahead(&store);
        assert!(ahead <= 100_000, "round {round}: {ahead} entries");
        out = s.refresh(&store);
    }
    let (_, ahead) = s.behind_ahead(&store);
    assert!(ahead <= 100_000, "after 5 rounds: {ahead}");
}

/// Pushes of records from many unknown authors cannot grow state past the bound.
#[test]
fn push_flood_of_unknown_authors_is_closed() {
    use perch_core::{Keypair, Unsigned, sign_record};
    let mut store = MemoryStore::new();
    let mut cfg = SessionConfig::new("a".repeat(32), Role::Node);
    cfg.max_extra_authors = 50;
    let mut s = Session::new(1, cfg, false);
    let _ = s.start();
    s.on_message(&mut store, &hello()).unwrap();
    s.on_message(&mut store, &Message::Heads { heads: vec![], next: None }.to_json()).unwrap();
    let mut closed = false;
    for i in 0..120u32 {
        let mut seed = [7u8; 32];
        seed[..4].copy_from_slice(&i.to_le_bytes());
        let k = Keypair::from_seed(seed);
        let body = serde_json::json!({ "n": i }).as_object().unwrap().clone();
        // seq 2 without seq 1: a gap, held but not stored.
        let r = sign_record(&k, Unsigned { seq: 2, prev: Some(perch_core::RecordId([1; 32])), ts: 1, kind: "note".into(), replaces: None, body }).unwrap();
        let m = Message::Push { records: vec![r.to_json_string()], evidence: vec![] }.to_json();
        if let Err(e) = s.on_message(&mut store, &m) {
            assert_eq!(e.code, ErrorCode::Misbehavior);
            closed = true;
            break;
        }
    }
    assert!(closed, "flood must close the session");
}
