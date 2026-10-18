//! # perch-store
//!
//! Storage for Perch records ([ARCHITECTURE](../../../ARCHITECTURE.md) section 5).
//!
//! | Item | Purpose |
//! |---|---|
//! | [`Store`] | Read and write interface used by views, sync, the node, and wasm |
//! | [`MemoryStore`] | In-memory implementation with indexes, in full or sparse mode |
//! | [`Insert`] | Outcome of inserting one record or one evidence pair |
//! | [`Change`] | Store changes, for sync fan-out and persistence |
//! | `JsonlFile` (native only) | Append-only JSON Lines persistence with replay |
//!
//! ## Modes
//!
//! - **Full** (default): logs are complete prefixes (5.1). `prev` continuity
//!   and non-decreasing `ts` are enforced. Out-of-order records are held in
//!   a bounded buffer and applied when the gap fills.
//! - **Sparse** (5.5): any isolated-valid record is stored. No `prev` or `ts`
//!   continuity is required. Forks are still detected between stored records.
//!
//! ## Forks (5.2)
//!
//! Each incoming record, and each record of an incoming evidence pair, is
//! compared with the stored records of its author at `seq − 1`, `seq`, and
//! `seq + 1` (prefix records and the stored pair). The lowest candidate pair
//! replaces the stored pair if it is lower. The log is then truncated to
//! `fork seq − 1`. Evidence records are stored without bodies.
//!
//! ## Determinism
//!
//! Every query returns results in a fixed order (IDs or keys ascending), so
//! views computed over a store are the same on every platform.

#![warn(missing_docs)]

mod index;
mod memory;
#[cfg(not(target_arch = "wasm32"))]
mod persist;

pub use memory::{AdmissionFn, AdmissionLimit, HoldLimits, IngestReport, MemoryStore, Mode};
#[cfg(not(target_arch = "wasm32"))]
pub use persist::{JsonlFile, ReplayReport};

pub use perch_core;

/// Splits a JSON array of records into one result per element. Each
/// element's exact text is checked against the 131,072-byte record limit and
/// then parsed strictly (`Record::from_json_str`: duplicate keys at any depth,
/// non-integer numbers, and lone surrogates are rejected). `Err` means the
/// text is not a JSON array.
pub fn parse_record_array(text: &str) -> Result<Vec<Result<Record, String>>, String> {
    let items: Vec<Box<serde_json::value::RawValue>> =
        serde_json::from_str(text).map_err(|e| format!("expected a JSON array of records: {e}"))?;
    Ok(items
        .iter()
        .map(|raw| {
            let t = raw.get();
            if t.len() > perch_core::limits::MAX_LINE_BYTES {
                return Err(format!("record is {} bytes; limit is {}", t.len(), perch_core::limits::MAX_LINE_BYTES));
            }
            Record::from_json_str(t).map_err(|e| e.to_string())
        })
        .collect())
}
use perch_core::{AuthorId, EvidencePair, ForkError, Hex32, Record, RecordError, RecordId};

/// The head of one author's log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Head {
    /// The author.
    pub author: AuthorId,
    /// Seq of the last stored prefix record; 0 if none.
    pub seq: u64,
    /// ID of that record; `None` when `seq` is 0.
    pub id: Option<RecordId>,
    /// Fork seq, if the author is forked.
    pub fork_seq: Option<u64>,
    /// IDs of the stored evidence pair `(a, b)`, if forked.
    pub evidence: Option<[RecordId; 2]>,
}

impl Head {
    /// True if the author has fork evidence.
    pub fn forked(&self) -> bool {
        self.fork_seq.is_some()
    }
}

/// Why a valid record was not stored.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Reject {
    /// `ts` is lower than the `ts` of the record at `seq − 1` (4.4).
    #[error("ts {ts} is lower than {prev_ts}, the ts of the record at seq - 1")]
    TsDecrease {
        /// `ts` of the previous record.
        prev_ts: u64,
        /// `ts` of this record.
        ts: u64,
    },
    /// The node's admission policy refused the record (section 9).
    #[error("admission: {0}")]
    Admission(String),
    /// A submitted pair does not prove a fork.
    #[error("not fork evidence: {0}")]
    NotEvidence(ForkError),
}

impl Reject {
    /// Short machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            Reject::TsDecrease { .. } => "ts_decrease",
            Reject::Admission(_) => "admission",
            Reject::NotEvidence(_) => "not_evidence",
        }
    }
}

/// Outcome of inserting one record or one evidence pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Insert {
    /// Stored: extended the author's prefix (full mode) or added (sparse mode).
    Inserted,
    /// Already held, with a body if this copy has one; nothing changed.
    Duplicate,
    /// Already held without a body; the body was added.
    BodyFilled,
    /// `seq` is above the next position. Held in the buffer (full mode) until the gap fills.
    Gap,
    /// The record (or pair) is fork evidence. `pair_changed` is true when the
    /// stored pair was created or replaced by a lower one.
    Fork {
        /// Whether the stored pair changed.
        pair_changed: bool,
    },
    /// The author is forked below this record's seq and the record cannot lower the pair.
    AboveFork,
    /// Refused.
    Rejected(Reject),
}

impl Insert {
    /// Short machine-readable name.
    pub fn name(&self) -> &'static str {
        match self {
            Insert::Inserted => "inserted",
            Insert::Duplicate => "duplicate",
            Insert::BodyFilled => "body_filled",
            Insert::Gap => "gap",
            Insert::Fork { .. } => "fork",
            Insert::AboveFork => "above_fork",
            Insert::Rejected(r) => r.code(),
        }
    }

    /// True when the store state changed.
    pub fn changed(&self) -> bool {
        matches!(self, Insert::Inserted | Insert::BodyFilled | Insert::Fork { pair_changed: true })
    }
}

/// A change to the store, reported by [`Store::drain_changes`] when change
/// tracking is on. Records drained from the hold buffer appear here too.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// A record was stored.
    Inserted(Record),
    /// A record held without a body received its body.
    BodyFilled(Record),
    /// An author's pair was created or lowered. The prefix may have been truncated.
    PairChanged(EvidencePair),
    /// A body was removed by [`Store::remove_body`].
    BodyRemoved(RecordId),
}

/// Errors of store operations other than inserts.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Control-kind bodies cannot be removed (ARCHITECTURE 4.5, 5.3).
    #[error("record {0} has a control kind; its body cannot be removed")]
    ControlKind(RecordId),
    /// A record could not be parsed or verified.
    #[error("invalid record: {0}")]
    Invalid(#[from] RecordError),
    /// File I/O failed.
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
}

/// The store interface. All lists are sorted ascending by ID unless noted.
pub trait Store {
    // ---- writes ----

    /// Inserts one isolated-valid record.
    fn insert(&mut self, record: Record) -> Insert;

    /// Inserts an evidence pair given as two records in any order.
    fn insert_evidence(&mut self, x: Record, y: Record) -> Insert;

    /// Drops the body of a record and adds its ID to the removal list.
    /// Refuses control kinds. Returns true if a stored body was dropped.
    fn remove_body(&mut self, id: &RecordId) -> Result<bool, StoreError>;

    /// Changes since the last call, in order. Empty unless tracking is on.
    fn drain_changes(&mut self) -> Vec<Change>;

    // ---- records ----

    /// A stored record by ID: prefix or sparse records, and evidence records (header only).
    fn get(&self, id: &RecordId) -> Option<Record>;

    /// True if [`Store::get`] would return a record.
    fn contains(&self, id: &RecordId) -> bool {
        self.get(id).is_some()
    }

    /// The body of a stored record, if held.
    fn body(&self, id: &RecordId) -> Option<serde_json::Map<String, serde_json::Value>> {
        self.get(id).and_then(|r| r.body().cloned())
    }

    /// Number of stored log records (prefix or sparse), excluding evidence-only records.
    fn len(&self) -> usize;

    /// True when no log record is stored.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// True for a sparse store (5.5).
    fn is_sparse(&self) -> bool;

    // ---- logs ----

    /// Authors with stored records or evidence, ascending.
    fn authors(&self) -> Vec<AuthorId>;

    /// Number of authors with stored records or evidence.
    fn author_count(&self) -> usize {
        self.authors().len()
    }

    /// The head of an author, or `None` if nothing is stored for them.
    fn head(&self, author: &AuthorId) -> Option<Head>;

    /// Up to `limit` heads of authors strictly greater than `after`, ascending.
    fn heads(&self, after: Option<&AuthorId>, limit: usize) -> Vec<Head>;

    /// Stored log records of an author with `seq ≥ from_seq`, ascending by seq, at most `limit`.
    fn log(&self, author: &AuthorId, from_seq: u64, limit: usize) -> Vec<Record>;

    /// The stored log record at `seq`.
    fn record_at(&self, author: &AuthorId, seq: u64) -> Option<Record>;

    /// The `(seq, prev)` an author's next record must use (publishing helper).
    fn next_position(&self, author: &AuthorId) -> (u64, Option<RecordId>) {
        perch_core::next_position(self.head(author).and_then(|h| h.id.map(|id| (h.seq, id))))
    }

    // ---- forks ----

    /// Fork seq of an author.
    fn fork_seq(&self, author: &AuthorId) -> Option<u64>;

    /// The stored evidence pair of an author (records without bodies).
    fn evidence(&self, author: &AuthorId) -> Option<EvidencePair>;

    /// Authors with fork evidence, ascending.
    fn forked_authors(&self) -> Vec<AuthorId>;

    // ---- indexes (log records only, never evidence-only records) ----

    /// IDs of records of one kind.
    fn by_kind(&self, kind: &str) -> Vec<RecordId>;

    /// IDs of records of one kind by one author, ascending by seq.
    fn by_author_kind(&self, author: &AuthorId, kind: &str) -> Vec<RecordId>;

    /// IDs of `product`, `listing`, `observation`, `review`, and `redact` records for a product key.
    ///
    /// `product` records are keyed by `derive_product_key(body, root)`. A
    /// `product` record without a body or with an invalid body is keyed `rec:<root>`.
    /// Other kinds are keyed by the raw `body.product` string (optional for `redact`).
    fn by_product(&self, key: &str) -> Vec<RecordId>;

    /// All product keys in the [`Store::by_product`] index, ascending.
    fn product_keys(&self) -> Vec<String>;

    /// Number of product keys in the [`Store::by_product`] index.
    fn product_key_count(&self) -> usize {
        self.product_keys().len()
    }

    /// IDs of `flag`, `appeal`, `retract`, and `redact` records whose raw `body.target` is `id`.
    fn by_target(&self, id: &RecordId) -> Vec<RecordId>;

    /// IDs of records that reference `id` by `body.listing`,
    /// `body.receipt.listing`, or header `replaces`.
    fn referencing(&self, id: &RecordId) -> Vec<RecordId>;

    /// IDs of records whose header `replaces` is `id`.
    fn replaced_by(&self, id: &RecordId) -> Vec<RecordId>;

    /// IDs of `attest.identity`, `attest.domain`, and `attest.revoke` records whose `body.subject` is `author`.
    fn by_subject(&self, author: &AuthorId) -> Vec<RecordId>;

    /// IDs of `attest.identity` records by `attestor` whose raw `body.person` is `person`.
    fn by_person(&self, attestor: &AuthorId, person: &Hex32) -> Vec<RecordId>;

    // ---- bodies ----

    /// Up to `limit` IDs, strictly greater than `after`, of log records held
    /// without a body and not on the removal list.
    fn missing_bodies(&self, after: Option<&RecordId>, limit: usize) -> Vec<RecordId>;

    /// Number of log records held without a body (for any reason).
    fn bodyless_count(&self) -> usize;

    /// True if `id` is on the removal list.
    fn is_removed(&self, id: &RecordId) -> bool;
}
