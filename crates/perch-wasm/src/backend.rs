//! The seam between this crate and perch-core, perch-store, perch-sync, and
//! perch-views. Every call into those crates goes through this file, so an
//! upstream API change touches only this file.
//!
//! Inputs and outputs are wire strings and `serde_json::Value`s in the
//! contract shapes (ARCHITECTURE 4, 7.8).

use crate::guard::HeadRef;
use perch_core::{AuthorId, Keypair, Record, RecordId, Unsigned};
use perch_store::{Insert, MemoryStore, Store};
use perch_sync::{Hub, Message, Outbox, Role, SessionConfig, State};
use perch_views::{Policy, SearchOptions, Sort};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::HashMap;

/// Result of offering one record to the store, in `ingest()` terms (8.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ingested {
    Inserted,
    Duplicate,
    Fork,
    Gap,
    Rejected { id: Option<String>, error: String },
}

/// A record this client signed.
#[derive(Clone, Debug)]
pub struct Signed {
    pub id: String,
    pub wire: String,
}

/// Fields of a record header needed by the client.
#[derive(Clone, Debug)]
pub struct Header {
    pub author: String,
    pub seq: u64,
    pub ts: u64,
    pub id: String,
}

pub type SessionId = u64;

/// Per-session numbers for `peers()`.
#[derive(Clone, Debug, Default)]
pub struct SessionInfo {
    /// The initial heads exchange is complete.
    pub live: bool,
    /// Authors whose head at the peer is lower than here, and the reverse.
    pub behind: u64,
    pub ahead: u64,
}

/// A frame to send, with the number of records it carries.
pub struct Outgoing {
    pub session: SessionId,
    pub text: String,
    pub records: u64,
}

/// A session the state machine wants closed.
pub struct CloseRequest {
    pub session: SessionId,
    pub reason: String,
}

pub struct Backend {
    sparse: bool,
    store: MemoryStore,
    hub: Hub,
    /// Frames and closes produced by the hub, waiting for the socket layer.
    out: Vec<Outgoing>,
    closes: Vec<CloseRequest>,
    /// Per session: the head the peer advertised for `watch` (a claim).
    claims: HashMap<SessionId, HeadRef>,
    /// The author whose head claims are tracked (the user's key).
    watch: Option<String>,
}

impl Backend {
    /// `node` is 32 hex characters, random per client (sync-protocol 3.2).
    pub fn new(sparse: bool, node: &str) -> Backend {
        let mut store = if sparse { MemoryStore::new_sparse() } else { MemoryStore::new() };
        store.set_track_changes(!sparse);
        let mut cfg = SessionConfig::new(node, Role::Browser);
        cfg.software = format!("perch-wasm/{}", env!("CARGO_PKG_VERSION"));
        Backend { sparse, store, hub: Hub::new(cfg), out: Vec::new(), closes: Vec::new(), claims: HashMap::new(), watch: None }
    }

    pub fn is_sparse(&self) -> bool {
        self.sparse
    }

    /// Tracks head claims for this author (the user's key).
    pub fn watch_author(&mut self, author: Option<&str>) {
        self.watch = author.map(String::from);
        self.claims.clear();
    }

    // Keys and signing.

    pub fn author_of(&self, secret: &[u8; 32]) -> String {
        Keypair::from_seed(*secret).author().to_hex()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn sign_record(
        &self,
        secret: &[u8; 32],
        kind: &str,
        body_json: &str,
        seq: u64,
        prev: Option<&str>,
        ts: u64,
        replaces: Option<&str>,
    ) -> Result<Signed, String> {
        let body: Map<String, Value> = match serde_json::from_str::<Value>(body_json) {
            Ok(Value::Object(m)) => m,
            Ok(_) => return Err("Body must be an object.".into()),
            Err(e) => return Err(format!("Body is not valid JSON: {e}")),
        };
        let prev = prev.map(parse_record_id).transpose()?;
        let replaces = replaces.map(parse_record_id).transpose()?;
        let kp = Keypair::from_seed(*secret);
        let rec = perch_core::sign_record(&kp, Unsigned { seq, prev, ts, kind: kind.to_string(), replaces, body })
            .map_err(|e| e.to_string())?;
        // Body schema errors are reported before the record is stored, so a
        // user does not publish a record that views will ignore.
        if let Err(e) = perch_core::check_record(&rec) {
            if perch_core::kinds::is_known_kind(kind) {
                return Err(format!("The {kind} body is invalid: {e}"));
            }
        }
        Ok(Signed { id: rec.id().to_hex(), wire: rec.to_json_string() })
    }

    pub fn issue_receipt(&self, secret: &[u8; 32], listing: &str, buyer: &str, purchased_at: u64, nonce: &[u8; 32]) -> Result<Value, String> {
        let listing = parse_record_id(listing)?;
        let buyer = AuthorId::from_hex(buyer).map_err(|_| "The buyer key must be 64 lowercase hex characters.".to_string())?;
        let kp = Keypair::from_seed(*secret);
        let r = perch_core::sign_receipt(&kp, listing, buyer, purchased_at, *nonce).map_err(|e| e.to_string())?;
        Ok(r.to_value())
    }

    /// `{ valid, seller, error }`. The seller is the author of the listing
    /// chain named in the receipt, which must be in the local store.
    pub fn verify_receipt(&self, receipt_json: &str) -> Value {
        let fail = |seller: Option<String>, e: String| json!({ "valid": false, "seller": seller, "error": e });
        let v: Value = match serde_json::from_str(receipt_json) {
            Ok(v) => v,
            Err(_) => return fail(None, "The receipt is not valid JSON.".into()),
        };
        let r = match perch_core::receipt::parse_receipt(&v) {
            Ok(r) => r,
            Err(e) => return fail(None, format!("Not a receipt: {e}.")),
        };
        let Some(seller) = self.listing_author(&r.listing) else {
            return fail(None, "The listing is not in your local data. Sync with a node and try again.".into());
        };
        match r.verify(&seller) {
            Ok(()) => json!({ "valid": true, "seller": seller.to_hex(), "error": null }),
            Err(_) => fail(Some(seller.to_hex()), "The signature does not match the listing's seller.".into()),
        }
    }

    /// `{ valid, id, error }`
    pub fn verify_record(&self, line: &str) -> Value {
        match Record::from_json_str(line) {
            Ok(r) => json!({ "valid": true, "id": r.id().to_hex(), "error": null }),
            Err(e) => {
                // Report the ID when the header parses, so users can match it.
                let id = header_id(line);
                json!({ "valid": false, "id": id, "error": e.to_string() })
            }
        }
    }

    // Store.

    pub fn insert(&mut self, line: &str) -> Ingested {
        self.insert_many(&[line.to_string()]).pop().unwrap_or(Ingested::Duplicate)
    }

    /// Verifies and stores records, then gossips the accepted ones to every
    /// session (local publish and ingest go through the hub, sync-protocol 5.2).
    pub fn insert_many(&mut self, lines: &[String]) -> Vec<Ingested> {
        let mut results: Vec<Option<Ingested>> = Vec::with_capacity(lines.len());
        let mut records = Vec::new();
        let mut slots = Vec::new();
        for line in lines {
            match Record::from_json_str(line) {
                Ok(r) => {
                    slots.push(results.len());
                    results.push(None);
                    records.push(r);
                }
                Err(e) => results.push(Some(Ingested::Rejected { id: header_id(line), error: e.to_string() })),
            }
        }
        if !records.is_empty() {
            let (outcomes, outbox) = self.hub.on_local(&mut self.store, records);
            self.queue(outbox);
            for (slot, o) in slots.into_iter().zip(outcomes) {
                let line = &lines[slot];
                results[slot] = Some(match o {
                    Insert::Inserted | Insert::BodyFilled => Ingested::Inserted,
                    Insert::Duplicate => Ingested::Duplicate,
                    Insert::Fork { .. } => Ingested::Fork,
                    Insert::Gap | Insert::AboveFork => Ingested::Gap,
                    Insert::Rejected(r) => Ingested::Rejected { id: header_id(line), error: r.to_string() },
                });
            }
        }
        results.into_iter().map(|r| r.unwrap_or(Ingested::Duplicate)).collect()
    }

    pub fn head(&self, author: &str) -> Option<HeadRef> {
        let a = AuthorId::from_hex(author).ok()?;
        let h = self.store.head(&a)?;
        h.id.map(|id| HeadRef { seq: h.seq, id: id.to_hex() })
    }

    pub fn header_of(&self, id: &str) -> Option<Header> {
        let r = self.store.get(&RecordId::from_hex(id).ok()?)?;
        Some(Header { author: r.author().to_hex(), seq: r.seq(), ts: r.ts(), id: r.id().to_hex() })
    }

    pub fn record(&self, id: &str) -> Option<String> {
        Some(self.store.get(&RecordId::from_hex(id).ok()?)?.to_json_string())
    }

    pub fn author_log(&self, author: &str) -> Vec<String> {
        let Ok(a) = AuthorId::from_hex(author) else { return Vec::new() };
        self.store.log(&a, 1, usize::MAX).iter().map(Record::to_json_string).collect()
    }

    pub fn export_jsonl(&self) -> String {
        self.store.export_jsonl()
    }

    fn listing_author(&self, listing: &RecordId) -> Option<AuthorId> {
        let r = self.store.get(listing)?;
        (r.kind() == "listing").then(|| r.author())
    }

    // Policy and views. Every view takes the policy as the app holds it, with
    // `now` already filled in by the client (client.rs effective_policy).

    pub fn check_policy(&self, policy: &Value) -> Result<(), String> {
        to_policy(policy).map(|_| ())
    }

    pub fn policy_hash(&self, policy: &Value) -> String {
        to_policy(policy).map(|p| perch_views::policy_hash(&p)).unwrap_or_default()
    }

    pub fn search(&self, policy: &Value, q: &str, sort: &str, currency: Option<&str>, limit: Option<usize>) -> Value {
        let Ok(p) = to_policy(policy) else { return json!([]) };
        let opts = SearchOptions {
            sort: Sort::parse(sort).unwrap_or_default(),
            currency: currency.map(String::from),
            limit: limit.unwrap_or(perch_views::DEFAULT_LIMIT).clamp(1, perch_views::MAX_LIMIT),
        };
        to_json(&perch_views::search(&self.store, &p, q, &opts))
    }

    pub fn product(&self, policy: &Value, key: &str) -> Value {
        let Ok(p) = to_policy(policy) else { return Value::Null };
        to_json(&perch_views::product_view(&self.store, &p, key))
    }

    pub fn listing(&self, policy: &Value, id: &str) -> Value {
        let (Ok(p), Ok(id)) = (to_policy(policy), RecordId::from_hex(id)) else { return Value::Null };
        to_json(&perch_views::listing_view(&self.store, &p, &id))
    }

    pub fn author_view(&self, policy: &Value, id: &str) -> Value {
        let (Ok(p), Ok(a)) = (to_policy(policy), AuthorId::from_hex(id)) else { return Value::Null };
        to_json(&perch_views::author_view(&self.store, &p, &a))
    }

    pub fn author_records(&self, policy: &Value, author: &str, kinds: Option<&[String]>, limit: Option<usize>, from_seq: Option<u64>) -> Value {
        let (Ok(p), Ok(a)) = (to_policy(policy), AuthorId::from_hex(author)) else { return json!([]) };
        let kinds: Vec<&str> = kinds.unwrap_or(&[]).iter().map(String::as_str).collect();
        to_json(&perch_views::author_records(&self.store, &p, &a, &kinds, limit.unwrap_or(usize::MAX), from_seq.unwrap_or(1)))
    }

    pub fn flags(&self, policy: &Value, root: &str) -> Value {
        let (Ok(p), Ok(id)) = (to_policy(policy), RecordId::from_hex(root)) else { return json!([]) };
        to_json(&perch_views::flags(&self.store, &p, &id))
    }

    pub fn record_status(&self, policy: &Value, id: &str) -> Value {
        let (Ok(p), Ok(id)) = (to_policy(policy), RecordId::from_hex(id)) else { return Value::Null };
        to_json(&perch_views::record_status(&self.store, &p, &id))
    }

    pub fn stats(&self) -> Value {
        to_json(&perch_views::stats(&self.store))
    }

    pub fn currencies(&self) -> Value {
        Value::Array(
            perch_core::currency::CURRENCIES
                .iter()
                .map(|c| json!({ "code": c.code, "minor_units": c.minor_units }))
                .collect(),
        )
    }

    // Sync sessions (perch-sync). One session per WebSocket connection.

    fn queue(&mut self, outbox: Outbox) {
        for (session, msg) in outbox.send {
            let records = msg.record_count() as u64;
            if let Message::Error { message, .. } = &msg {
                if outbox.close.contains(&session) && !self.closes.iter().any(|c| c.session == session) {
                    self.closes.push(CloseRequest { session, reason: message.clone() });
                }
            }
            self.out.push(Outgoing { session, text: msg.to_json(), records });
        }
        for session in outbox.close {
            if !self.closes.iter().any(|c| c.session == session) {
                self.closes.push(CloseRequest { session, reason: "closed by the sync protocol".into() });
            }
        }
    }

    /// A WebSocket opened: start a session (queues `hello`).
    pub fn open_session(&mut self, _url: &str) -> SessionId {
        let (id, hello) = self.hub.open(true);
        for m in hello {
            self.out.push(Outgoing { session: id, text: m.to_json(), records: 0 });
        }
        id
    }

    /// Handles one inbound frame. Returns the number of records it carried
    /// and whether the store changed.
    pub fn on_message(&mut self, sid: SessionId, text: &str) -> (u64, bool) {
        self.note_claim(sid, text);
        let before = self.fingerprint();
        let received = self.hub.session(sid).map_or(0, |s| s.stats.records_received);
        let outbox = self.hub.on_message(&mut self.store, sid, text);
        self.queue(outbox);
        let after = self.hub.session(sid).map_or(received, |s| s.stats.records_received);
        (after.saturating_sub(received), self.fingerprint() != before)
    }

    /// A cheap summary that changes whenever the store changes.
    fn fingerprint(&self) -> (usize, usize, usize) {
        (self.store.len(), self.store.bodyless_count(), self.store.forked_authors().len())
    }

    /// Records the head a peer advertises for the watched author. The claim
    /// is unverified; the publish guard treats it as such (guard.rs).
    fn note_claim(&mut self, sid: SessionId, text: &str) {
        let Some(author) = &self.watch else { return };
        if !text.contains(author.as_str()) || !text.contains("\"heads\"") {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(text) else { return };
        if v.get("type").and_then(Value::as_str) != Some("heads") {
            return;
        }
        for e in v.get("heads").and_then(Value::as_array).into_iter().flatten() {
            if e.get("author").and_then(Value::as_str) != Some(author.as_str()) {
                continue;
            }
            let seq = e.get("seq").and_then(Value::as_u64).unwrap_or(0);
            match e.get("id").and_then(Value::as_str) {
                Some(id) if seq > 0 => {
                    self.claims.insert(sid, HeadRef { seq, id: id.to_string() });
                }
                _ => {
                    self.claims.remove(&sid);
                }
            }
        }
    }

    pub fn on_closed(&mut self, sid: SessionId) {
        self.hub.close(sid);
        self.claims.remove(&sid);
        self.out.retain(|o| o.session != sid);
        self.closes.retain(|c| c.session != sid);
    }

    /// Frames to send and sessions to close, in order.
    pub fn take_outgoing(&mut self) -> (Vec<Outgoing>, Vec<CloseRequest>) {
        (std::mem::take(&mut self.out), std::mem::take(&mut self.closes))
    }

    pub fn session_info(&self, sid: SessionId) -> SessionInfo {
        let Some(s) = self.hub.session(sid) else { return SessionInfo::default() };
        let (behind, ahead) = s.behind_ahead(&self.store);
        SessionInfo { live: s.state() == State::Live, behind: behind as u64, ahead: ahead as u64 }
    }

    /// The head the peer claims for the watched author, if it advertised one.
    pub fn peer_head(&self, sid: SessionId, author: &str) -> Option<HeadRef> {
        if self.watch.as_deref() != Some(author) {
            return None;
        }
        self.claims.get(&sid).cloned()
    }

    /// Starts a new heads exchange on every session.
    pub fn refresh(&mut self) {
        let outbox = self.hub.refresh(&self.store);
        self.queue(outbox);
    }
}

/// The app's policy JSON as a `perch_views::Policy`. `now: null` becomes 0
/// here; views always receive a filled-in `now` from the client.
fn to_policy(v: &Value) -> Result<Policy, String> {
    let mut v = v.clone();
    if !v.is_object() {
        return Err("Policy must be an object.".into());
    }
    if v.get("now").is_none_or(Value::is_null) {
        v["now"] = json!(0);
    }
    // Null optional fields are the same as absent ones (ARCHITECTURE 7.1).
    if let Some(list) = v.get_mut("trusted_attestors").and_then(Value::as_array_mut) {
        for a in list {
            if let Some(m) = a.as_object_mut() {
                m.retain(|_, x| !x.is_null());
            }
        }
    }
    Policy::from_value(v).map_err(|e| e.to_string())
}

fn to_json<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

fn parse_record_id(s: &str) -> Result<RecordId, String> {
    RecordId::from_hex(s).map_err(|_| format!("\"{s}\" is not a record ID (64 lowercase hex characters)."))
}

/// ID of a record whose header parses even if the record is invalid.
fn header_id(line: &str) -> Option<String> {
    let v: Value = serde_json::from_str(line).ok()?;
    let m = v.as_object()?;
    let mut h = Map::new();
    for k in ["v", "author", "seq", "prev", "ts", "kind", "replaces", "body_hash"] {
        if let Some(x) = m.get(k) {
            h.insert(k.to_string(), x.clone());
        }
    }
    let c = perch_core::canonical::map_to_canonical_json(&h).ok()?;
    Some(perch_core::ids::encode_hex(&perch_core::sha256(c.as_bytes())))
}
