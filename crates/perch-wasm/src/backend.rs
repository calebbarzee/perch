//! The seam between this crate and perch-core, perch-store, perch-sync, and
//! perch-views. Every call into those crates goes through this file, so an
//! upstream API change touches only this file.
//!
//! Inputs and outputs are wire strings and `serde_json::Value`s in the
//! contract shapes (ARCHITECTURE 4, 7.8).

use crate::guard::HeadRef;
use perch_core::{AuthorId, Keypair, Record, RecordId, Unsigned};
use perch_store::{Insert, MemoryStore, Store};
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
    store: MemoryStore,
}


impl Backend {
    pub fn new(sparse: bool) -> Backend {
        let store = if sparse { MemoryStore::new_sparse() } else { MemoryStore::new() };
        Backend { sparse, store }
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
        match self.store.insert_json(line) {
            Err(e) => Ingested::Rejected { id: header_id(line), error: e.to_string() },
            Ok(Insert::Inserted | Insert::BodyFilled) => Ingested::Inserted,
            Ok(Insert::Duplicate) => Ingested::Duplicate,
            Ok(Insert::Fork { .. }) => Ingested::Fork,
            Ok(Insert::Gap | Insert::AboveFork) => Ingested::Gap,
            Ok(Insert::Rejected(r)) => Ingested::Rejected { id: header_id(line), error: r.to_string() },
        }
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
        json!({ "records": self.store.len(), "authors": self.store.authors().len(), "products": self.store.product_keys().len(),
                "listings": 0, "reviews": 0, "forks": self.store.forked_authors().len(), "invalid": 0,
                "redacted": self.store.bodyless_count(), "unknown": 0 })
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
