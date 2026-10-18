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
