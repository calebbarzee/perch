//! Audit #6 and its re-checks: per-session state is bounded by the local
//! author count plus the want window, whatever the peer does, and honest
//! sync from a peer of any size completes.

use perch_core::{Keypair, Record, Unsigned, sign_record};
use perch_store::{MemoryStore, Store};
use perch_sync::message::ErrorCode;
use perch_sync::sim::Sim;
use perch_sync::{Message, Role, Session, SessionConfig, WANT_WINDOW};

fn hex(n: u64, salt: u64) -> String {
    perch_core::ids::encode_hex(&perch_core::sha256(&[n.to_le_bytes(), salt.to_le_bytes()].concat()))
}

fn hello() -> String {
    format!(
        r#"{{"type":"hello","protocol":"perch-sync","versions":[1],"node":"{}","role":"node","software":"evil"}}"#,
        "b".repeat(32)
    )
}

fn page(authors: &[String], next: bool) -> String {
    let entries: Vec<String> =
        authors.iter().map(|a| format!(r#"{{"author":"{a}","seq":5,"id":"{}"}}"#, "c".repeat(64))).collect();
    let next = if next { format!(r#","next":"{}""#, authors.last().unwrap()) } else { String::new() };
    format!(r#"{{"type":"heads","heads":[{}]{next}}}"#, entries.join(","))
}

/// A peer that answers every get_heads with fresh authors and never answers
/// wants: the session stops pulling pages once the window is full.
#[test]
fn endless_heads_paging_is_bounded() {
    let mut store = MemoryStore::new();
    let mut s = Session::new(1, SessionConfig::new("a".repeat(32), Role::Node), false);
    let _ = s.start();
    let mut out = s.on_message(&mut store, &hello()).unwrap();
    let mut pages = 0u64;
    for round in 0..200u64 {
        if !out.iter().any(|m| matches!(m, Message::GetHeads { .. })) {
            break;
        }
        let authors: Vec<String> = (0..4096u64).map(|i| format!("{:064x}", round * 4096 + i + 1)).collect();
        out = s.on_message(&mut store, &page(&authors, true)).unwrap();
        pages += 1;
        assert!(s.state_size() <= WANT_WINDOW, "state {}", s.state_size());
    }
    assert!(!s.is_closed());
    assert!(pages <= (WANT_WINDOW / 4096) as u64 + 1, "pulled {pages} pages without answers");
    // Refreshes do not restart an exchange that waits for room.
    for _ in 0..5 {
        assert!(!s.refresh(&store).iter().any(|m| matches!(m, Message::GetHeads { .. })));
    }
    assert!(s.state_size() <= WANT_WINDOW);
}

/// The auditor's refresh probe (sync_mem2): state does not accumulate across rounds.
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
        for (i, p) in pages.iter().enumerate() {
            if !out.iter().any(|m| matches!(m, Message::GetHeads { .. })) {
                break;
            }
            out = s.on_message(&mut store, &page(p, i + 1 < pages.len())).unwrap_or_else(|e| panic!("round {round} page {i}: {e}"));
        }
        assert!(s.state_size() <= WANT_WINDOW, "round {round}: {}", s.state_size());
        out = s.refresh(&store);
    }
    assert!(!s.is_closed());
}

#[test]
fn heads_pages_must_advance() {
    let mut store = MemoryStore::new();
    let mut s = Session::new(1, SessionConfig::new("a".repeat(32), Role::Node), false);
    let _ = s.start();
    s.on_message(&mut store, &hello()).unwrap();
    let a = "5".repeat(64);
    let p = format!(r#"{{"type":"heads","heads":[{{"author":"{a}","seq":1,"id":"{}"}}],"next":"{a}"}}"#, "c".repeat(64));
    s.on_message(&mut store, &p).unwrap();
    // Repeating the same page after cursor `a` is refused.
    let e = s.on_message(&mut store, &p).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadMessage);
    // An empty page with `next` is refused.
    let mut s = Session::new(1, SessionConfig::new("a".repeat(32), Role::Node), false);
    let _ = s.start();
    s.on_message(&mut store, &hello()).unwrap();
    let e = s.on_message(&mut store, &format!(r#"{{"type":"heads","heads":[],"next":"{a}"}}"#)).unwrap_err();
    assert_eq!(e.code, ErrorCode::BadMessage);
}

/// Pushes of gap records from many unknown authors stay within the window.
#[test]
fn push_flood_of_unknown_authors_is_bounded() {
    let mut store = MemoryStore::new();
    let mut cfg = SessionConfig::new("a".repeat(32), Role::Node);
    cfg.want_window = 50;
    let mut s = Session::new(1, cfg, false);
    let _ = s.start();
    s.on_message(&mut store, &hello()).unwrap();
    s.on_message(&mut store, &Message::Heads { heads: vec![], next: None }.to_json()).unwrap();
    for i in 0..300u32 {
        let mut seed = [7u8; 32];
        seed[..4].copy_from_slice(&i.to_le_bytes());
        let k = Keypair::from_seed(seed);
        let body = serde_json::json!({ "n": i }).as_object().unwrap().clone();
        // seq 2 without seq 1: a gap, held but not stored.
        let r = sign_record(&k, Unsigned { seq: 2, prev: Some(perch_core::RecordId([1; 32])), ts: 1, kind: "note".into(), replaces: None, body })
            .unwrap();
        let m = Message::Push { records: vec![r.to_json_string()], evidence: vec![] }.to_json();
        s.on_message(&mut store, &m).unwrap();
        assert!(s.state_size() <= 50, "{}", s.state_size());
    }
}

/// One light record per author.
fn author_records(n: usize) -> Vec<Record> {
    (0..n)
        .map(|i| {
            let mut seed = [3u8; 32];
            seed[..8].copy_from_slice(&(i as u64).to_le_bytes());
            let k = Keypair::from_seed(seed);
            let body = serde_json::json!({}).as_object().unwrap().clone();
            sign_record(&k, Unsigned { seq: 1, prev: None, ts: 1, kind: "n".into(), replaces: None, body }).unwrap()
        })
        .collect()
}

/// An empty node syncs fully from an honest peer with `n` authors, with
/// session state within the local author count plus the window throughout.
fn sync_from_large_peer(n: usize) {
    let t0 = std::time::Instant::now();
    let mut sim = Sim::new(2, 9);
    for r in author_records(n) {
        sim.nodes[0].store.insert(r);
    }
    sim.nodes[0].store.drain_changes();
    let signed = t0.elapsed();
    sim.connect(1, 0);
    let mut max_extra = 0usize;
    let mut steps = 0u64;
    while sim.step() {
        steps += 1;
        let local = sim.nodes[1].store.author_count();
        for s in sim.nodes[1].hub.sessions() {
            max_extra = max_extra.max(s.state_size().saturating_sub(local));
            assert!(s.state_size() <= local + WANT_WINDOW, "state {} with {local} local authors", s.state_size());
        }
    }
    assert_eq!(sim.nodes[1].store.author_count(), n, "all authors synced");
    sim.check_converged(&[0, 1]).unwrap();
    println!(
        "synced {n} authors in {:.1} s ({:.1} s to sign), {steps} messages, {} bytes; state beyond local authors at most {max_extra}",
        t0.elapsed().as_secs_f64(),
        signed.as_secs_f64(),
        sim.stats.bytes
    );
}

#[test]
fn empty_node_syncs_from_20k_author_peer() {
    sync_from_large_peer(20_000);
}

/// 150,000 authors. Run with `cargo test -p perch-sync --test sync_mem -- --ignored --nocapture`.
#[test]
#[ignore]
fn empty_node_syncs_from_150k_author_peer() {
    sync_from_large_peer(150_000);
}
