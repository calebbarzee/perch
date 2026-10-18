//! [`Author`]: a key plus its log position, for writing valid logs.

use perch_core::serde_json::{Map, Value};
use perch_core::{AuthorId, Keypair, Record, RecordId, Unsigned, sha256, sign_record};

/// A key and the state of its log (next seq, previous ID, last `ts`).
///
/// [`Author::publish`] produces records that pass isolated and log-level
/// validity: seq increases by 1, `prev` links, and `ts` never decreases.
/// Body schemas are not checked, so tests can write semantically invalid
/// records on purpose.
#[derive(Debug, Clone)]
pub struct Author {
    keypair: Keypair,
    next_seq: u64,
    head: Option<RecordId>,
    head_prev: Option<RecordId>,
    last_ts: u64,
}

impl Author {
    /// A new author with an empty log.
    pub fn new(keypair: Keypair) -> Author {
        Author { keypair, next_seq: 1, head: None, head_prev: None, last_ts: 0 }
    }

    /// A new author whose key seed is `sha256("perch-demo/v1/" || label)`.
    /// The secret key is therefore public: use demo keys only for demos and tests.
    pub fn from_label(label: &str) -> Author {
        Author::new(keypair_for_label(label))
    }

    /// The author ID (public key).
    pub fn id(&self) -> AuthorId {
        self.keypair.author()
    }

    /// The key pair.
    pub fn keypair(&self) -> &Keypair {
        &self.keypair
    }

    /// Seq of the next record.
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// ID of the last record, if any.
    pub fn head(&self) -> Option<RecordId> {
        self.head
    }

    /// `ts` of the last record (0 if none).
    pub fn last_ts(&self) -> u64 {
        self.last_ts
    }

    /// Signs and appends a record. `ts` is raised to the last `ts` if lower,
    /// so the log stays valid.
    ///
    /// # Panics
    /// If `body` is not a JSON object or the record breaks an isolated rule
    /// (for example size limits).
    pub fn publish(&mut self, ts: u64, kind: &str, body: Value, replaces: Option<RecordId>) -> Record {
        let r = self.sign_at(self.next_seq, self.head, ts, kind, body, replaces);
        self.next_seq += 1;
        self.head_prev = self.head;
        self.head = Some(r.id());
        self.last_ts = r.ts();
        r
    }

    /// Signs a second record at the position of the last published record
    /// (same seq and `prev`) without changing the log state. Together with
    /// that record it is fork evidence (ARCHITECTURE 5.2, rule 1).
    ///
    /// # Panics
    /// If nothing has been published yet, or as for [`Author::publish`].
    pub fn publish_conflicting(&self, ts: u64, kind: &str, body: Value, replaces: Option<RecordId>) -> Record {
        assert!(self.next_seq > 1, "publish_conflicting needs a published record");
        self.sign_at(self.next_seq - 1, self.head_prev, ts, kind, body, replaces)
    }

    fn sign_at(
        &self,
        seq: u64,
        prev: Option<RecordId>,
        ts: u64,
        kind: &str,
        body: Value,
        replaces: Option<RecordId>,
    ) -> Record {
        let ts = ts.max(self.last_ts);
        sign_record(&self.keypair, Unsigned { seq, prev, ts, kind: kind.to_owned(), replaces, body: body_map(body) })
            .unwrap_or_else(|e| panic!("sign_record failed: {e:?}"))
    }
}

/// The demo key pair for `label`: seed `sha256("perch-demo/v1/" || label)`.
pub fn keypair_for_label(label: &str) -> Keypair {
    Keypair::from_seed(sha256(format!("perch-demo/v1/{label}").as_bytes()))
}

/// Converts a `json!` object into a body map.
///
/// # Panics
/// If `v` is not an object.
pub fn body_map(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => panic!("body must be a JSON object"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perch_core::EvidencePair;
    use perch_core::serde_json::json;

    #[test]
    fn log_links_and_fork() {
        let mut a = Author::from_label("t");
        let r1 = a.publish(10, "profile", json!({"name": "T", "role": "buyer"}), None);
        let r2 = a.publish(5, "profile", json!({"name": "T2", "role": "buyer"}), None);
        assert_eq!(r2.seq(), 2);
        assert_eq!(r2.prev(), Some(r1.id()));
        assert_eq!(r2.ts(), 10, "ts raised to keep the log non-decreasing");
        let f = a.publish_conflicting(11, "profile", json!({"name": "T3", "role": "buyer"}), None);
        assert_eq!((f.seq(), f.prev()), (2, Some(r1.id())));
        assert_eq!(EvidencePair::new(r2, f).unwrap().fork_seq(), 2);
    }
}
