//! The seam between this crate and perch-core, perch-store, perch-sync, and
//! perch-views. Every call into those crates goes through this file, so an
//! upstream API change touches only this file.
//!
//! Inputs and outputs are wire strings and `serde_json::Value`s in the
//! contract shapes (ARCHITECTURE 4, 7.8).

use crate::guard::HeadRef;
use perch_core::{AuthorId, Keypair, Record, RecordId, Unsigned};
use serde_json::{Map, Value, json};

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

pub struct Backend {
    sparse: bool,
    store: interim::Store,
}


impl Backend {
    pub fn new(sparse: bool) -> Backend {
        Backend { sparse, store: interim::Store::new(sparse) }
    }

    pub fn is_sparse(&self) -> bool {
        self.sparse
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
        // Body schema errors are reported before signing is final, so a user
        // does not publish a record that views will ignore.
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
        let rec = match Record::from_json_str(line) {
            Ok(r) => r,
            Err(e) => return Ingested::Rejected { id: header_id(line), error: e.to_string() },
        };
        self.store.insert(rec)
    }

    pub fn head(&self, author: &str) -> Option<HeadRef> {
        let a = AuthorId::from_hex(author).ok()?;
        self.store.head(&a).map(|(seq, id)| HeadRef { seq, id: id.to_hex() })
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
        self.store.log(&a).iter().map(Record::to_json_string).collect()
    }

    pub fn export_jsonl(&self) -> String {
        perch_core::to_jsonl(self.store.all())
    }

    fn listing_author(&self, listing: &RecordId) -> Option<AuthorId> {
        let r = self.store.get(listing)?;
        (r.kind() == "listing").then(|| r.author())
    }

    // Views. `policy` has `now` set.

    pub fn check_policy(&self, _policy: &Value) -> Result<(), String> {
        Ok(())
    }

    pub fn policy_hash(&self, _policy: &Value) -> String {
        String::new()
    }

    pub fn search(&self, _policy: &Value, _q: &str, _sort: &str, _currency: Option<&str>) -> Value {
        Value::Array(Vec::new())
    }

    pub fn product(&self, _policy: &Value, _key: &str) -> Value {
        Value::Null
    }

    pub fn listing(&self, _policy: &Value, _id: &str) -> Value {
        Value::Null
    }

    pub fn author_view(&self, _policy: &Value, _id: &str) -> Value {
        Value::Null
    }

    pub fn author_records(&self, _policy: &Value, _author: &str, _kinds: Option<&[String]>, _limit: Option<usize>, _from_seq: Option<u64>) -> Value {
        Value::Array(Vec::new())
    }

    pub fn flags(&self, _policy: &Value, _root: &str) -> Value {
        Value::Array(Vec::new())
    }

    pub fn record_status(&self, _policy: &Value, _id: &str) -> Value {
        Value::Null
    }

    pub fn stats(&self, _policy: &Value) -> Value {
        let (records, authors, forks, redacted) = self.store.counts();
        json!({ "records": records, "authors": authors, "products": 0, "listings": 0, "reviews": 0,
                "forks": forks, "invalid": 0, "redacted": redacted, "unknown": 0 })
    }

    pub fn currencies(&self) -> Value {
        Value::Array(
            perch_core::currency::CURRENCIES
                .iter()
                .map(|c| json!({ "code": c.code, "minor_units": c.minor_units }))
                .collect(),
        )
    }
}

// Sync sessions (perch-sync). One session per WebSocket connection.

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

#[derive(Default)]
pub struct Sessions {
    next: SessionId,
    open: Vec<SessionId>,
}

impl Backend {
    pub fn open_session(&mut self, sessions: &mut Sessions, _url: &str) -> SessionId {
        sessions.next += 1;
        sessions.open.push(sessions.next);
        sessions.next
    }

    /// Handles one inbound frame. Returns the number of records it carried
    /// and whether the store changed.
    pub fn on_message(&mut self, _sessions: &mut Sessions, _sid: SessionId, _text: &str) -> (u64, bool) {
        (0, false)
    }

    pub fn on_closed(&mut self, sessions: &mut Sessions, sid: SessionId) {
        sessions.open.retain(|s| *s != sid);
    }

    /// Frames to send, in order, across all sessions.
    pub fn take_outgoing(&mut self, _sessions: &mut Sessions) -> (Vec<Outgoing>, Vec<CloseRequest>) {
        (Vec::new(), Vec::new())
    }

    pub fn session_info(&self, _sessions: &Sessions, _sid: SessionId) -> SessionInfo {
        SessionInfo::default()
    }

    /// The peer's head for `author`, as known from its heads and pushes.
    /// `None` until the peer reported it or when the peer lacks the author.
    pub fn peer_head(&self, _sessions: &Sessions, _sid: SessionId, _author: &str) -> Option<HeadRef> {
        None
    }
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

/// Interim store until perch-store lands: complete prefixes per author,
/// gap buffer, one evidence pair per author with prefix truncation. It
/// implements only what the client needs and is replaced by
/// `perch_store::MemoryStore`.
mod interim {
    use super::Ingested;
    use perch_core::{AuthorId, EvidencePair, Record, RecordId};
    use std::collections::{BTreeMap, HashMap};

    pub struct Store {
        sparse: bool,
        logs: BTreeMap<AuthorId, Vec<Record>>,
        by_id: HashMap<RecordId, (AuthorId, u64)>,
        held: BTreeMap<(AuthorId, u64), Record>,
        evidence: BTreeMap<AuthorId, EvidencePair>,
        sparse_recs: BTreeMap<AuthorId, BTreeMap<u64, Record>>,
    }

    impl Store {
        pub fn new(sparse: bool) -> Store {
            Store { sparse, logs: BTreeMap::new(), by_id: HashMap::new(), held: BTreeMap::new(), evidence: BTreeMap::new(), sparse_recs: BTreeMap::new() }
        }

        pub fn head(&self, a: &AuthorId) -> Option<(u64, RecordId)> {
            if self.sparse {
                let (seq, r) = self.sparse_recs.get(a)?.iter().next_back()?;
                return Some((*seq, r.id()));
            }
            let log = self.logs.get(a)?;
            log.last().map(|r| (r.seq(), r.id()))
        }

        pub fn get(&self, id: &RecordId) -> Option<&Record> {
            if let Some((a, seq)) = self.by_id.get(id) {
                if self.sparse {
                    return self.sparse_recs.get(a)?.get(seq);
                }
                if let Some(r) = self.logs.get(a).and_then(|l| l.get((*seq - 1) as usize)) {
                    if r.id() == *id {
                        return Some(r);
                    }
                }
            }
            self.evidence.values().flat_map(|p| p.records().into_iter()).find(|r| r.id() == *id)
        }

        pub fn log(&self, a: &AuthorId) -> Vec<Record> {
            if self.sparse {
                return self.sparse_recs.get(a).map(|m| m.values().cloned().collect()).unwrap_or_default();
            }
            self.logs.get(a).cloned().unwrap_or_default()
        }

        pub fn all(&self) -> impl Iterator<Item = &Record> {
            self.logs
                .values()
                .flatten()
                .chain(self.sparse_recs.values().flat_map(|m| m.values()))
                .chain(self.evidence.values().flat_map(|p| p.records().into_iter()))
        }

        pub fn counts(&self) -> (u64, u64, u64, u64) {
            let records = self.all().count() as u64;
            let mut authors: Vec<&AuthorId> = self.logs.keys().chain(self.sparse_recs.keys()).collect();
            authors.sort();
            authors.dedup();
            let redacted = self.all().filter(|r| !r.has_body()).count() as u64;
            (records, authors.len() as u64, self.evidence.len() as u64, redacted)
        }

        pub fn insert(&mut self, rec: Record) -> Ingested {
            let id = rec.id();
            let a = rec.author();
            if let Some(existing) = self.get(&id) {
                if !existing.has_body() && rec.has_body() {
                    let (a2, seq) = self.by_id[&id];
                    if self.sparse {
                        self.sparse_recs.get_mut(&a2).unwrap().insert(seq, rec);
                    } else {
                        self.logs.get_mut(&a2).unwrap()[(seq - 1) as usize] = rec;
                    }
                    return Ingested::Inserted;
                }
                return Ingested::Duplicate;
            }
            if self.sparse {
                let m = self.sparse_recs.entry(a).or_default();
                if let Some(other) = m.get(&rec.seq()) {
                    if let Ok(pair) = EvidencePair::new(other.clone(), rec.clone()) {
                        self.set_pair(pair);
                        return Ingested::Fork;
                    }
                }
                self.by_id.insert(id, (a, rec.seq()));
                m.insert(rec.seq(), rec);
                return Ingested::Inserted;
            }
            if let Some(p) = self.evidence.get(&a) {
                if rec.seq() >= p.fork_seq() + 2 {
                    return Ingested::Duplicate;
                }
            }
            let log = self.logs.entry(a).or_default();
            let len = log.len() as u64;
            let seq = rec.seq();
            // Conflicts with stored records at seq - 1, seq, seq + 1.
            let mut candidates = Vec::new();
            for s in [seq.saturating_sub(1), seq, seq + 1] {
                if s >= 1 && s <= len {
                    if let Ok(p) = EvidencePair::new(log[(s - 1) as usize].clone(), rec.clone()) {
                        candidates.push(p);
                    }
                }
            }
            if let Some(best) = candidates.into_iter().min() {
                self.set_pair(best);
                return Ingested::Fork;
            }
            if seq <= len {
                return Ingested::Duplicate;
            }
            if seq > len + 1 {
                if self.held.len() < 4096 {
                    self.held.insert((a, seq), rec);
                }
                return Ingested::Gap;
            }
            if let Some(prev) = log.last() {
                if rec.ts() < prev.ts() {
                    return Ingested::Rejected { id: Some(id.to_hex()), error: "ts is lower than the previous record in the log".into() };
                }
            }
            self.by_id.insert(id, (a, seq));
            log.push(rec);
            // Release held records that now continue the log.
            let mut next = seq + 1;
            while let Some(r) = self.held.remove(&(a, next)) {
                match self.insert(r) {
                    Ingested::Inserted => next += 1,
                    _ => break,
                }
            }
            Ingested::Inserted
        }

        fn set_pair(&mut self, pair: EvidencePair) {
            let a = pair.author();
            let lower = self.evidence.get(&a).is_none_or(|old| pair < *old);
            if !lower {
                return;
            }
            let keep = pair.fork_seq().saturating_sub(1) as usize;
            if let Some(log) = self.logs.get_mut(&a) {
                for r in log.drain(keep.min(log.len())..) {
                    self.by_id.remove(&r.id());
                }
            }
            self.evidence.insert(a, pair);
        }
    }
}
