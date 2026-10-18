//! The client logic behind `PerchClient`, as plain Rust: keys, the publish
//! guard, ingest, policy, and views. No JavaScript types, so it is tested
//! natively. `lib.rs` adds the JavaScript bindings and `net.rs` the sockets.

use crate::backend::{Backend, Ingested, RecordInput, Signed, parse_strict};
use crate::backup;
use crate::guard::{Guard, HeadRef, PublishStatus};
use serde_json::{Value, json};

pub struct Client {
    pub backend: Backend,
    pub guard: Guard,
    secret: Option<[u8; 32]>,
    author: Option<String>,
    policy: Value,
    rejected_total: u64,
}

/// Counts returned by `ingest()` (ARCHITECTURE 8.2).
#[derive(Default, Debug)]
pub struct IngestReport {
    pub inserted: u64,
    pub duplicate: u64,
    pub fork: u64,
    pub gap: u64,
    pub rejected: u64,
    pub errors: Vec<Value>,
}

impl IngestReport {
    pub fn to_json(&self) -> Value {
        json!({
            "inserted": self.inserted, "duplicate": self.duplicate, "fork": self.fork,
            "gap": self.gap, "rejected": self.rejected, "errors": self.errors,
        })
    }

    pub fn changed(&self) -> bool {
        self.inserted > 0 || self.fork > 0
    }
}

pub fn default_policy() -> Value {
    json!({
        "now": null,
        "trusted_attestors": [],
        "flag_threshold": 3,
        "flag_thresholds": { "review": 5, "product": 5 },
        "flags_require_verified": true,
        "seller_hidden_listing_limit": 3,
        "seller_min_distinct_flaggers": 9,
        "verified_purchase_requires_verified_buyer": true,
        "price_requires_domain_verified": true,
        "future_ts_tolerance_ms": 600000
    })
}

/// Splits `ingest()` input text into records: JSONL lines, or the elements
/// of one JSON array. The array is parsed with the strict parser, which
/// rejects duplicate keys anywhere in it; each element is then checked
/// against the per-record size limit and verified (backend.rs).
pub fn split_records(text: &str) -> Result<Vec<RecordInput>, String> {
    let trimmed = text.trim_start();
    if trimmed.starts_with('[') {
        let v = parse_strict(trimmed).map_err(|e| format!("Not a valid JSON array: {e}"))?;
        let Value::Array(arr) = v else { return Err("Not a valid JSON array".into()) };
        return Ok(arr.into_iter().map(RecordInput::Value).collect());
    }
    Ok(text.lines().map(str::trim).filter(|l| !l.is_empty()).map(|l| RecordInput::Text(l.to_string())).collect())
}

impl Client {
    /// A client with a fixed node ID (tests). Browsers use `with_node`.
    pub fn new(sparse: bool) -> Client {
        Client::with_node(sparse, &"0".repeat(32))
    }

    /// `node`: 32 hex characters, random per client (sync-protocol 3.2).
    pub fn with_node(sparse: bool, node: &str) -> Client {
        Client {
            backend: Backend::new(sparse, node),
            guard: Guard::new(sparse),
            secret: None,
            author: None,
            policy: default_policy(),
            rejected_total: 0,
        }
    }

    // Keys.

    pub fn generate_key(&mut self, seed: [u8; 32]) -> String {
        self.secret = Some(seed);
        let a = self.backend.author_of(&seed);
        self.author = Some(a.clone());
        self.backend.watch_author(Some(&a));
        self.guard.reset_for_new_key();
        a
    }

    pub fn import_key(&mut self, input: &str) -> Result<String, String> {
        let b = backup::parse(input)?;
        let a = self.backend.author_of(&b.secret);
        self.secret = Some(b.secret);
        self.author = Some(a.clone());
        self.backend.watch_author(Some(&a));
        self.guard.reset_for_import(b.head);
        if let Some(h) = self.backend.head(&a) {
            self.guard.note_known(h);
        }
        Ok(a)
    }

    pub fn export_key(&mut self) -> Result<String, String> {
        let secret = self.secret.ok_or("No key. Generate or import a key first.")?;
        self.refresh_own_head();
        Ok(backup::format(&secret, self.guard.known()))
    }

    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    fn refresh_own_head(&mut self) {
        if let Some(a) = &self.author {
            if let Some(h) = self.backend.head(a) {
                self.guard.note_known(h);
            }
        }
    }

    pub fn own_local_head(&self) -> Option<HeadRef> {
        self.author.as_ref().and_then(|a| self.backend.head(a))
    }

    pub fn publish_status(&mut self, now_ms: u64) -> PublishStatus {
        self.refresh_own_head();
        let local = self.own_local_head();
        self.guard.status(self.secret.is_some(), local, now_ms)
    }

    /// Signs and stores a record. `now_ms` is the wall clock; `ts` overrides it.
    pub fn publish(&mut self, kind: &str, body_json: &str, replaces: Option<&str>, ts: Option<u64>, now_ms: u64) -> Result<Signed, String> {
        let status = self.publish_status(now_ms);
        if !status.ready {
            return Err(status.reason.unwrap_or_else(|| "Publishing is paused.".into()));
        }
        let secret = self.secret.ok_or("No key.")?;
        let author = self.author.clone().ok_or("No key.")?;
        let head = self.backend.head(&author);
        let (seq, prev, prev_ts) = match &head {
            Some(h) => (h.seq + 1, Some(h.id.clone()), self.backend.header_of(&h.id).map_or(0, |x| x.ts)),
            None => (1, None, 0),
        };
        // ts must not decrease along the log (ARCHITECTURE 4.4).
        let ts = ts.unwrap_or(now_ms).max(prev_ts);
        let signed = self.backend.sign_record(&secret, kind, body_json, seq, prev.as_deref(), ts, replaces)?;
        match self.backend.insert(&signed.wire) {
            Ingested::Inserted => {}
            other => return Err(format!("The signed record was not stored: {other:?}")),
        }
        self.guard.note_known(HeadRef { seq, id: signed.id.clone() });
        Ok(signed)
    }

    pub fn issue_receipt(&self, listing: &str, buyer: &str, purchased_at: u64, nonce: [u8; 32]) -> Result<Value, String> {
        let secret = self.secret.ok_or("No key. Generate or import a key first.")?;
        self.backend.issue_receipt(&secret, listing, buyer, purchased_at, &nonce)
    }

    // Records.

    pub fn ingest_lines(&mut self, lines: &[String]) -> IngestReport {
        let inputs: Vec<RecordInput> = lines.iter().cloned().map(RecordInput::Text).collect();
        self.ingest(&inputs)
    }

    pub fn ingest(&mut self, inputs: &[RecordInput]) -> IngestReport {
        let mut r = IngestReport::default();
        for (index, outcome) in self.backend.insert_inputs(inputs).into_iter().enumerate() {
            match outcome {
                Ingested::Inserted => r.inserted += 1,
                Ingested::Duplicate => r.duplicate += 1,
                Ingested::Fork => r.fork += 1,
                Ingested::Gap => r.gap += 1,
                Ingested::Rejected { id, error } => {
                    r.rejected += 1;
                    if r.errors.len() < 1000 {
                        r.errors.push(json!({ "index": index, "id": id, "error": error }));
                    }
                }
            }
        }
        self.rejected_total += r.rejected;
        if r.changed() {
            self.refresh_own_head();
        }
        r
    }

    // Policy.

    pub fn set_policy(&mut self, policy: Value) -> Result<(), String> {
        if !policy.is_object() {
            return Err("Policy must be an object.".into());
        }
        self.backend.check_policy(&policy)?;
        self.policy = policy;
        Ok(())
    }

    pub fn policy(&self) -> &Value {
        &self.policy
    }

    pub fn policy_hash(&self) -> String {
        self.backend.policy_hash(&self.policy)
    }

    /// The policy with `now` filled in: the pinned value, or the wall clock.
    pub fn effective_policy(&self, now_ms: u64) -> Value {
        let mut p = self.policy.clone();
        if p.get("now").is_none_or(Value::is_null) {
            p["now"] = json!(now_ms);
        }
        p
    }

    pub fn rejected_total(&self) -> u64 {
        self.rejected_total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(v: Vec<RecordInput>) -> Vec<String> {
        v.into_iter()
            .map(|i| match i {
                RecordInput::Text(t) => t,
                RecordInput::Value(v) => v.to_string(),
            })
            .collect()
    }

    #[test]
    fn split_jsonl_and_array() {
        assert_eq!(texts(split_records("{\"a\":1}\n\n {\"b\":2}\n").unwrap()), vec!["{\"a\":1}", "{\"b\":2}"]);
        assert_eq!(texts(split_records("[{\"a\":1},{\"b\":2}]").unwrap()), vec!["{\"a\":1}", "{\"b\":2}"]);
        assert!(split_records("[1,").is_err());
    }

    #[test]
    fn array_with_duplicate_keys_or_floats_is_rejected() {
        let e = split_records("[{\"a\":1,\"a\":2}]").err().unwrap();
        assert!(e.contains("duplicate"), "{e}");
        assert!(split_records("[{\"a\":1.5}]").is_err());
        assert!(split_records("[{\"a\":9007199254740992}]").is_err());
    }

    #[test]
    fn effective_policy_fills_now_only_when_unpinned() {
        let mut c = Client::new(false);
        assert_eq!(c.effective_policy(5)["now"], json!(5));
        let mut p = default_policy();
        p["now"] = json!(7);
        c.set_policy(p).unwrap();
        assert_eq!(c.effective_policy(5)["now"], json!(7));
    }

    #[test]
    fn export_requires_key() {
        let mut c = Client::new(false);
        assert!(c.export_key().is_err());
        let a = c.generate_key([7u8; 32]);
        assert_eq!(a.len(), 64);
        let b = c.export_key().unwrap();
        assert!(b.contains("\"head\":null"));
        let mut d = Client::new(false);
        assert_eq!(d.import_key(&b).unwrap(), a);
    }

    #[test]
    fn imported_key_waits_for_nodes() {
        let mut c = Client::new(false);
        c.import_key(&"11".repeat(32)).unwrap();
        assert!(!c.publish_status(0).ready);
    }

    #[test]
    fn sparse_client_refuses_publish() {
        let mut c = Client::new(true);
        c.generate_key([1u8; 32]);
        let err = c.publish("profile", "{\"name\":\"x\",\"role\":\"buyer\"}", None, None, 1).unwrap_err();
        assert!(err.contains("sparse"));
    }
}
