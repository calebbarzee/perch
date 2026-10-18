//! The client's sync sessions against a perch-sync node hub over an
//! in-memory transport, and views through perch-views on demo data.

use perch_core::Record;
use perch_store::{MemoryStore, Store};
use perch_sync::{Hub, Role, SessionConfig};
use perch_wasm::client::Client;
use serde_json::{Value, json};

const T: u64 = 1_760_000_000_000;

fn demo() -> perch_demo::Demo {
    perch_demo::generate_demo(7, perch_demo::Size::Small)
}

fn demo_policy(d: &perch_demo::Demo) -> Value {
    let p = perch_views::default_policy(d.now, &d.attestors);
    let mut v = serde_json::to_value(&p).unwrap();
    v["now"] = Value::Null;
    v
}

/// A node: hub + store, connected to the client's session `sid`.
struct Node {
    hub: Hub,
    store: MemoryStore,
    id: u64,
}

impl Node {
    fn new(records: &[Record]) -> Node {
        let mut store = MemoryStore::new();
        store.set_track_changes(true);
        let mut hub = Hub::new(SessionConfig::new("1".repeat(32), Role::Node));
        let _ = hub.on_local(&mut store, records.to_vec());
        Node { hub, store, id: 0 }
    }
}

/// Moves frames both ways until both sides are quiet. Returns frames moved.
fn pump(c: &mut Client, sid: u64, n: &mut Node, first: Vec<String>) -> usize {
    let mut to_node = first;
    let mut moved = 0;
    for _ in 0..10_000 {
        let mut to_client = Vec::new();
        for text in to_node.drain(..) {
            moved += 1;
            let out = n.hub.on_message(&mut n.store, n.id, &text);
            to_client.extend(out.send.into_iter().filter(|(id, _)| *id == n.id).map(|(_, m)| m.to_json()));
        }
        for text in &to_client {
            moved += 1;
            c.backend.on_message(sid, text);
        }
        let (out, _) = c.backend.take_outgoing();
        to_node = out.into_iter().filter(|o| o.session == sid).map(|o| o.text).collect();
        if to_node.is_empty() && to_client.is_empty() {
            break;
        }
    }
    moved
}

fn connect(c: &mut Client, n: &mut Node) -> u64 {
    let sid = c.backend.open_session("ws://node.test/v1/sync");
    let (nid, hello) = n.hub.open(false);
    n.id = nid;
    let (out, _) = c.backend.take_outgoing();
    // Node hello to the client first, then the client's frames to the node.
    for m in hello {
        c.backend.on_message(sid, &m.to_json());
    }
    let mut first: Vec<String> = out.into_iter().map(|o| o.text).collect();
    let (more, _) = c.backend.take_outgoing();
    first.extend(more.into_iter().map(|o| o.text));
    pump(c, sid, n, first);
    sid
}

#[test]
fn client_syncs_from_a_node_and_gossips_its_publish() {
    let d = demo();
    let mut node = Node::new(&d.records);
    let mut c = Client::new(false);
    let sid = connect(&mut c, &mut node);
    let info = c.backend.session_info(sid);
    assert!(info.live, "session not live");
    assert_eq!(c.backend.export_jsonl(), node.store.export_jsonl(), "client store differs from node");
    assert_eq!((info.behind, info.ahead), (0, 0));

    c.generate_key([21; 32]);
    let s = c.publish("profile", &json!({ "name": "Sync test", "role": "buyer" }).to_string(), None, Some(T), T).unwrap();
    let (out, _) = c.backend.take_outgoing();
    pump(&mut c, sid, &mut node, out.into_iter().map(|o| o.text).collect());
    let id = perch_core::RecordId::from_hex(&s.id).unwrap();
    assert!(node.store.get(&id).is_some(), "node did not receive the published record");
}

#[test]
fn node_head_claim_reaches_the_guard_input() {
    let d = demo();
    let mut a = Client::new(false);
    a.generate_key([22; 32]);
    let p = a.publish("profile", &json!({ "name": "Claim", "role": "buyer" }).to_string(), None, Some(T), T).unwrap();
    let mut records = d.records.clone();
    records.push(Record::from_json_str(&p.wire).unwrap());
    let mut node = Node::new(&records);

    let mut b = Client::new(false);
    b.import_key(&"16".repeat(32)).unwrap();
    let author = b.author().unwrap().to_string();
    let sid = connect(&mut b, &mut node);
    let claim = b.backend.peer_head(sid, &author).expect("node advertised the author's head");
    assert_eq!(claim.seq, 1);
    assert_eq!(b.backend.head(&author).unwrap().seq, 1, "record synced to the device");
}

#[test]
fn views_run_through_perch_views() {
    let d = demo();
    let mut c = Client::new(false);
    let lines: Vec<String> = d.records.iter().map(Record::to_json_string).collect();
    let r = c.ingest_lines(&lines);
    assert_eq!(r.rejected, 0, "{:?}", r.errors.first());
    c.set_policy(demo_policy(&d)).unwrap();
    let p = c.effective_policy(d.now);
    let hits = c.backend.search(&p, "", "relevance", None, Some(1000));
    let hits = hits.as_array().unwrap();
    assert!(!hits.is_empty());
    let key = hits[0]["key"].as_str().unwrap();
    let view = c.backend.product(&p, key);
    assert_eq!(view["key"], json!(key));
    assert!(view["ratings"]["verified_identity"]["count"].is_u64());
    let stats = c.backend.stats();
    assert!(stats["records"].as_u64().unwrap() > 0);
    assert_eq!(c.policy_hash().len(), 64);
    // Pinned now gives the same result as calling perch-views directly.
    let direct = perch_views::product_view(&{
        let mut s = MemoryStore::new();
        for rec in &d.records {
            s.insert(rec.clone());
        }
        s
    }, &perch_views::default_policy(d.now, &d.attestors), key);
    assert_eq!(perch_views::canonical_json(&direct), perch_views::canonical_json(&view));
}
