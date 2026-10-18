//! Deterministic simulation: many peers, each with a [`MemoryStore`] and a
//! [`Hub`], connected by in-memory message queues.
//!
//! - Each connection delivers messages in order (as WebSocket over TCP does).
//!   Delivery order across connections is random (seeded).
//! - `push` messages can be duplicated in flight.
//! - Links can be cut and re-established (partitions that heal).
//! - A peer can be set to strip bodies from everything it sends.
//!
//! Used by this crate's tests and available to `perch-tests`.

use std::collections::{BTreeMap, VecDeque};

use perch_core::{AuthorId, Keypair, Record, RecordId, Unsigned, sign_record};
use perch_store::{MemoryStore, Store};
use serde_json::{Map, Value, json};

use crate::hub::Hub;
use crate::message::{Message, Role};
use crate::session::SessionConfig;

/// SplitMix64: small, seedable, platform-independent.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n` (`n > 0`).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// True with probability `p`.
    pub fn chance(&mut self, p: f64) -> bool {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }
}

/// One simulated peer.
pub struct SimNode {
    pub store: MemoryStore,
    pub hub: Hub,
    /// Peer node index → session ID.
    pub sessions: BTreeMap<usize, u64>,
    /// When true, every record this peer sends loses its body.
    pub strip_bodies: bool,
}

/// Traffic counters.
#[derive(Clone, Debug, Default)]
pub struct SimStats {
    pub messages: u64,
    pub bytes: u64,
    /// Per message type: (messages, bytes).
    pub by_type: BTreeMap<String, (u64, u64)>,
    pub dropped_on_close: u64,
}

/// A simulated author (client) that signs records and submits them.
pub struct SimAuthor {
    pub key: Keypair,
    pub seq: u64,
    pub prev: Option<RecordId>,
    pub ts: u64,
}

/// The simulation.
pub struct Sim {
    pub nodes: Vec<SimNode>,
    links: BTreeMap<(usize, usize), VecDeque<String>>,
    pub rng: Rng,
    pub stats: SimStats,
    pub authors: Vec<SimAuthor>,
    /// Probability of duplicating a `push` message in flight.
    pub dup_push: f64,
    pub closed_sessions: u64,
}

fn node_id(i: usize, seed: u64) -> String {
    format!("{:016x}{:016x}", seed, i as u64 + 1)
}

fn strip(text: &str) -> String {
    let Ok(m) = Message::parse(text) else { return text.to_string() };
    let s = |v: Vec<String>| -> Vec<String> {
        v.into_iter()
            .map(|r| Record::from_json_str(&r).map(|r| r.without_body().to_json_string()).unwrap_or(r))
            .collect()
    };
    let m = match m {
        Message::Records { records, truncated } => Message::Records { records: s(records), truncated },
        Message::Push { records, evidence } => Message::Push { records: s(records), evidence },
        Message::Bodies { records, truncated } => Message::Bodies { records: s(records), truncated },
        other => other,
    };
    m.to_json()
}

impl Sim {
    /// `n` peers with empty stores and no connections.
    pub fn new(n: usize, seed: u64) -> Sim {
        let mut sim = Sim {
            nodes: Vec::new(),
            links: BTreeMap::new(),
            rng: Rng::new(seed),
            stats: SimStats::default(),
            authors: Vec::new(),
            dup_push: 0.0,
            closed_sessions: 0,
        };
        for _ in 0..n {
            sim.add_node();
        }
        sim
    }

    /// Adds a peer with an empty store. Returns its index.
    pub fn add_node(&mut self) -> usize {
        let i = self.nodes.len();
        let mut store = MemoryStore::new();
        store.set_track_changes(true);
        let seed = self.rng.next_u64();
        let cfg = SessionConfig::new(node_id(i, seed), Role::Node);
        self.nodes.push(SimNode { store, hub: Hub::new(cfg), sessions: BTreeMap::new(), strip_bodies: false });
        i
    }

    /// Adds `n` simulated authors with keys from `seed`.
    pub fn add_authors(&mut self, n: usize, seed: u8) {
        for i in 0..n {
            let mut s = [seed; 32];
            s[0] = i as u8;
            s[1] = (i >> 8) as u8;
            self.authors.push(SimAuthor { key: Keypair::from_seed(s), seq: 0, prev: None, ts: 1_760_000_000_000 });
        }
    }

    fn enqueue(&mut self, from: usize, to: usize, msg: &Message) {
        let mut text = msg.to_json();
        if self.nodes[from].strip_bodies {
            text = strip(&text);
        }
        let dup = matches!(msg, Message::Push { .. }) && self.dup_push > 0.0 && self.rng.chance(self.dup_push);
        let q = self.links.entry((from, to)).or_default();
        q.push_back(text.clone());
        if dup {
            q.push_back(text);
        }
    }

    fn route(&mut self, from: usize, out: crate::hub::Outbox) {
        let peers: BTreeMap<u64, usize> = self.nodes[from].sessions.iter().map(|(p, s)| (*s, *p)).collect();
        for (sid, msg) in &out.send {
            if let Some(&to) = peers.get(sid) {
                self.enqueue(from, to, msg);
            }
        }
        for sid in out.close {
            if let Some(&to) = peers.get(&sid) {
                self.disconnect(from, to);
            }
        }
    }

    pub fn connected(&self, a: usize, b: usize) -> bool {
        self.nodes[a].sessions.contains_key(&b)
    }

    /// Opens a connection; `a` dials `b`.
    pub fn connect(&mut self, a: usize, b: usize) {
        if a == b || self.connected(a, b) {
            return;
        }
        let (sa, ha) = self.nodes[a].hub.open(true);
        let (sb, hb) = self.nodes[b].hub.open(false);
        self.nodes[a].sessions.insert(b, sa);
        self.nodes[b].sessions.insert(a, sb);
        for m in ha {
            self.enqueue(a, b, &m);
        }
        for m in hb {
            self.enqueue(b, a, &m);
        }
    }

    /// Closes a connection; messages in flight are lost.
    pub fn disconnect(&mut self, a: usize, b: usize) {
        for (x, y) in [(a, b), (b, a)] {
            if let Some(sid) = self.nodes[x].sessions.remove(&y) {
                self.nodes[x].hub.close(sid);
                self.closed_sessions += 1;
            }
            if let Some(q) = self.links.remove(&(x, y)) {
                self.stats.dropped_on_close += q.len() as u64;
            }
        }
    }

    /// Delivers one message on a random non-empty link. Returns false when all queues are empty.
    pub fn step(&mut self) -> bool {
        let busy: Vec<(usize, usize)> = self.links.iter().filter(|(_, q)| !q.is_empty()).map(|(k, _)| *k).collect();
        if busy.is_empty() {
            return false;
        }
        let (from, to) = busy[self.rng.below(busy.len())];
        let Some(text) = self.links.get_mut(&(from, to)).and_then(|q| q.pop_front()) else { return true };
        self.stats.messages += 1;
        self.stats.bytes += text.len() as u64;
        let ty = text
            .strip_prefix("{\"type\":\"")
            .and_then(|r| r.split('"').next())
            .unwrap_or("invalid")
            .to_string();
        let e = self.stats.by_type.entry(ty).or_default();
        e.0 += 1;
        e.1 += text.len() as u64;
        let Some(&sid) = self.nodes[to].sessions.get(&from) else { return true };
        let node = &mut self.nodes[to];
        let out = node.hub.on_message(&mut node.store, sid, &text);
        self.route(to, out);
        true
    }

    /// Runs until no message is in flight or `max_steps` is reached. Returns steps taken.
    pub fn run(&mut self, max_steps: u64) -> u64 {
        let mut n = 0;
        while n < max_steps && self.step() {
            n += 1;
        }
        n
    }

    /// Signs the next record of author `a` (kind `note`, small body).
    pub fn sign_next(&mut self, a: usize) -> Record {
        let au = &mut self.authors[a];
        au.seq += 1;
        au.ts += 1000;
        let mut body = Map::new();
        body.insert("text".into(), json!(format!("record {} of author {a}", au.seq)));
        body.insert("n".into(), Value::from(au.seq));
        let r = sign_record(&au.key, Unsigned { seq: au.seq, prev: au.prev, ts: au.ts, kind: "note".into(), replaces: None, body })
            .expect("sign");
        au.prev = Some(r.id());
        r
    }

    /// Submits records to a peer as a local client would (HTTP).
    pub fn submit(&mut self, node: usize, records: Vec<Record>) {
        let n = &mut self.nodes[node];
        let (_, out) = n.hub.on_local(&mut n.store, records);
        self.route(node, out);
    }

    /// Author `a` publishes one record at `node`.
    pub fn publish(&mut self, node: usize, a: usize) -> Record {
        let r = self.sign_next(a);
        self.submit(node, vec![r.clone()]);
        r
    }

    /// Author `a` signs two different records at the same seq and submits one to each peer.
    pub fn equivocate(&mut self, a: usize, node_x: usize, node_y: usize) -> (Record, Record) {
        let au = &mut self.authors[a];
        au.seq += 1;
        au.ts += 1000;
        let mk = |tag: &str, au: &SimAuthor| {
            let mut body = Map::new();
            body.insert("text".into(), json!(tag));
            sign_record(&au.key, Unsigned { seq: au.seq, prev: au.prev, ts: au.ts, kind: "note".into(), replaces: None, body })
                .expect("sign")
        };
        let x = mk("version x", au);
        let y = mk("version y", au);
        au.prev = Some(x.id());
        self.submit(node_x, vec![x.clone()]);
        self.submit(node_y, vec![y.clone()]);
        (x, y)
    }

    /// Starts a new heads exchange on every session of every peer.
    pub fn refresh_all(&mut self) {
        for i in 0..self.nodes.len() {
            let n = &mut self.nodes[i];
            let out = n.hub.refresh(&n.store);
            self.route(i, out);
        }
    }

    /// Connects peers in a line 0–1–2–….
    pub fn line(&mut self) {
        for i in 1..self.nodes.len() {
            self.connect(i - 1, i);
        }
    }

    /// Connects peers in a ring.
    pub fn ring(&mut self) {
        self.line();
        let n = self.nodes.len();
        if n > 2 {
            self.connect(n - 1, 0);
        }
    }

    /// Connects every peer to peer 0.
    pub fn star(&mut self) {
        for i in 1..self.nodes.len() {
            self.connect(i, 0);
        }
    }

    /// A random connected graph: a random spanning tree plus `extra` random edges.
    pub fn random_graph(&mut self, extra: usize) {
        let n = self.nodes.len();
        for i in 1..n {
            let j = self.rng.below(i);
            self.connect(i, j);
        }
        for _ in 0..extra {
            let a = self.rng.below(n);
            let b = self.rng.below(n);
            self.connect(a, b);
        }
    }

    /// Edges currently connected, each once as `(low, high)`.
    pub fn edges(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (a, n) in self.nodes.iter().enumerate() {
            for &b in n.sessions.keys() {
                if a < b {
                    out.push((a, b));
                }
            }
        }
        out
    }

    /// Summary of one peer's state: heads, evidence IDs, and IDs held without a body.
    pub fn fingerprint(&self, i: usize) -> NodeState {
        let s = &self.nodes[i].store;
        let heads = s.heads(None, usize::MAX).into_iter().map(|h| (h.author, h.seq, h.id)).collect();
        let pairs = s.forked_authors().into_iter().filter_map(|a| s.evidence(&a).map(|p| (a, p.ids()))).collect();
        let bodyless = s.missing_bodies(None, usize::MAX);
        NodeState { heads, pairs, bodyless }
    }

    /// Ok if every listed peer has the same state; Err describes the first difference.
    pub fn check_converged(&self, peers: &[usize]) -> Result<NodeState, String> {
        let Some(&first) = peers.first() else { return Err("no peers".into()) };
        let base = self.fingerprint(first);
        for &p in &peers[1..] {
            let f = self.fingerprint(p);
            if f.heads != base.heads {
                let diff: Vec<String> = base
                    .heads
                    .iter()
                    .zip(f.heads.iter())
                    .filter(|(a, b)| a != b)
                    .take(3)
                    .map(|(a, b)| format!("{}: seq {} vs {}", a.0, a.1, b.1))
                    .collect();
                return Err(format!(
                    "heads differ between peer {first} ({} authors) and peer {p} ({} authors): {diff:?}",
                    base.heads.len(),
                    f.heads.len()
                ));
            }
            if f.pairs != base.pairs {
                return Err(format!("evidence differs between peer {first} and peer {p}"));
            }
            if f.bodyless != base.bodyless {
                return Err(format!(
                    "bodies differ between peer {first} ({} missing) and peer {p} ({} missing)",
                    base.bodyless.len(),
                    f.bodyless.len()
                ));
            }
        }
        Ok(base)
    }

    pub fn all(&self) -> Vec<usize> {
        (0..self.nodes.len()).collect()
    }
}

/// See [`Sim::fingerprint`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeState {
    pub heads: Vec<(AuthorId, u64, Option<RecordId>)>,
    pub pairs: Vec<(AuthorId, [RecordId; 2])>,
    pub bodyless: Vec<RecordId>,
}
