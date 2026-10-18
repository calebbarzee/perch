//! [`MemoryStore`]: in-memory logs, evidence, indexes, hold buffer, and
//! optional JSONL persistence (native only).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;
use std::sync::Arc;

use perch_core::fork::fork_type;
use perch_core::kinds::{self, names};
use perch_core::{AuthorId, EvidencePair, Hex32, Record, RecordError, RecordId};

use crate::index::{IndexKeys, Indexes, keys_for};
use crate::{Change, Head, Insert, Reject, Store, StoreError};

/// Storage mode (module docs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Complete prefixes, `prev` and `ts` continuity (ARCHITECTURE 5.1).
    Full,
    /// Any isolated-valid record; no continuity checks (ARCHITECTURE 5.5).
    Sparse,
}

/// Limits of the buffer for out-of-order records (full mode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoldLimits {
    /// Records in the buffer, all authors.
    pub max_records: usize,
    /// Approximate bytes in the buffer, all authors.
    pub max_bytes: usize,
    /// Records in the buffer per author.
    pub per_author: usize,
}

impl Default for HoldLimits {
    fn default() -> Self {
        HoldLimits { max_records: 4096, max_bytes: 32 * 1024 * 1024, per_author: 256 }
    }
}

/// Per-author storage limit from the admission policy (ARCHITECTURE 9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdmissionLimit {
    /// Maximum records in the log.
    pub records: u64,
    /// Maximum approximate wire bytes of the log.
    pub bytes: u64,
    /// `Some(rank)` puts the author's log in the shared pool (tier 3). The
    /// pool's total bytes stay within [`MemoryStore::set_pool_budget`]. When
    /// the pool is full, logs with a higher `(rank, author)` are dropped
    /// whole to make room for a lower one; a record that would need a lower
    /// ranked log dropped is refused.
    pub pool_rank: Option<[u8; 32]>,
}

impl AdmissionLimit {
    /// No limit.
    pub const UNLIMITED: AdmissionLimit = AdmissionLimit { records: u64::MAX, bytes: u64::MAX, pool_rank: None };
}

/// Admission function: limit for an author given the current store.
pub type AdmissionFn = Arc<dyn Fn(&MemoryStore, &AuthorId) -> AdmissionLimit + Send + Sync>;

/// Counts of an ingest call, matching `PerchClient.ingest` (ARCHITECTURE 8.2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IngestReport {
    /// Records stored, including body fills.
    pub inserted: usize,
    /// Records already held.
    pub duplicate: usize,
    /// Records that were fork evidence.
    pub fork: usize,
    /// Records held for a gap, or above a fork.
    pub gap: usize,
    /// Records that were invalid or refused.
    pub rejected: usize,
    /// `(line number, error)` for each invalid or refused record.
    pub errors: Vec<(usize, String)>,
    /// Outcome per non-blank input line, in order; `Err` for invalid lines.
    pub outcomes: Vec<Result<Insert, String>>,
}

impl IngestReport {
    fn add(&mut self, line: usize, o: Result<Insert, String>) {
        match &o {
            Ok(Insert::Inserted | Insert::BodyFilled) => self.inserted += 1,
            Ok(Insert::Duplicate) => self.duplicate += 1,
            Ok(Insert::Fork { .. }) => self.fork += 1,
            Ok(Insert::Gap | Insert::AboveFork) => self.gap += 1,
            Ok(Insert::Rejected(r)) => {
                self.rejected += 1;
                self.errors.push((line, r.to_string()));
            }
            Err(e) => {
                self.rejected += 1;
                self.errors.push((line, e.clone()));
            }
        }
        self.outcomes.push(o);
    }
}

#[derive(Clone, Debug)]
struct Entry {
    record: Record,
    keys: IndexKeys,
}

#[derive(Clone, Debug, Default)]
struct Log {
    /// seq → ID. Full mode: keys are exactly 1..=k.
    recs: BTreeMap<u64, RecordId>,
    pair: Option<EvidencePair>,
    bytes: u64,
}

type HoldKey = (AuthorId, u64, RecordId);

/// In-memory store. See the crate docs.
#[derive(Clone)]
pub struct MemoryStore {
    mode: Mode,
    logs: BTreeMap<AuthorId, Log>,
    entries: BTreeMap<RecordId, Entry>,
    evidence_ids: BTreeMap<RecordId, AuthorId>,
    idx: Indexes,
    bodyless: BTreeSet<RecordId>,
    removed: BTreeSet<RecordId>,
    held: BTreeMap<HoldKey, (Record, u64)>,
    held_order: BTreeMap<u64, HoldKey>,
    held_per_author: BTreeMap<AuthorId, usize>,
    held_bytes: usize,
    hold_limits: HoldLimits,
    counter: u64,
    track: bool,
    changes: Vec<Change>,
    admission: Option<AdmissionFn>,
    pool_budget: u64,
    pool_bytes: u64,
    pool: BTreeMap<AuthorId, [u8; 32]>,
    pool_order: BTreeSet<([u8; 32], AuthorId)>,
    /// Lowest `(rank, author)` dropped or refused for budget since the last
    /// rebuild. Higher ranks are refused, so the pool always holds a prefix
    /// of the rank order.
    pool_cutoff: Option<([u8; 32], AuthorId)>,
    /// Sparse mode: per author, the lowest seq at which held records show a
    /// `ts` decrease. Records at or above it are ignored, as a full store
    /// would never reach them (ARCHITECTURE 4.4).
    ts_cut: BTreeMap<AuthorId, u64>,
    /// Set when a body was dropped because of the removal list, so the
    /// persistence file still holds it and must be rewritten.
    pub(crate) needs_compaction: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) jslot: JournalSlot,
}

/// The persistence file of a store. A clone of a store has no file.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Default)]
pub(crate) struct JournalSlot {
    pub file: Option<crate::persist::JsonlFile>,
    pub error: Option<std::io::Error>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Clone for JournalSlot {
    fn clone(&self) -> Self {
        JournalSlot::default()
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        MemoryStore::with_mode(Mode::Full)
    }
}

impl std::fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryStore")
            .field("mode", &self.mode)
            .field("authors", &self.logs.len())
            .field("records", &self.entries.len())
            .field("held", &self.held.len())
            .finish()
    }
}

/// Approximate wire size of a record, for limits.
pub(crate) fn wire_len(r: &Record) -> usize {
    r.header_canonical().len() + 160 + r.body_canonical().map_or(0, |b| b.len() + 8)
}

impl MemoryStore {
    /// An empty full-mode store.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty sparse-mode store.
    pub fn new_sparse() -> Self {
        Self::with_mode(Mode::Sparse)
    }

    /// An empty store in the given mode.
    pub fn with_mode(mode: Mode) -> Self {
        MemoryStore {
            mode,
            logs: BTreeMap::new(),
            entries: BTreeMap::new(),
            evidence_ids: BTreeMap::new(),
            idx: Indexes::default(),
            bodyless: BTreeSet::new(),
            removed: BTreeSet::new(),
            held: BTreeMap::new(),
            held_order: BTreeMap::new(),
            held_per_author: BTreeMap::new(),
            held_bytes: 0,
            hold_limits: HoldLimits::default(),
            counter: 0,
            track: false,
            changes: Vec::new(),
            admission: None,
            pool_budget: u64::MAX,
            pool_bytes: 0,
            pool: BTreeMap::new(),
            pool_order: BTreeSet::new(),
            pool_cutoff: None,
            ts_cut: BTreeMap::new(),
            needs_compaction: false,
            #[cfg(not(target_arch = "wasm32"))]
            jslot: JournalSlot::default(),
        }
    }

    /// The store's mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Turns change tracking on or off ([`Store::drain_changes`]).
    pub fn set_track_changes(&mut self, on: bool) {
        self.track = on;
        if !on {
            self.changes.clear();
        }
    }

    /// Sets the hold buffer limits.
    pub fn set_hold_limits(&mut self, limits: HoldLimits) {
        self.hold_limits = limits;
    }

    /// Sets the admission function. `None` admits everything.
    pub fn set_admission(&mut self, f: Option<AdmissionFn>) {
        self.admission = f;
    }

    /// Adds IDs to the removal list before records arrive (for example at
    /// startup). Bodies of these records are refused, except for control kinds.
    pub fn add_removed(&mut self, ids: impl IntoIterator<Item = RecordId>) {
        self.removed.extend(ids);
    }

    /// The removal list, ascending.
    pub fn removed_ids(&self) -> Vec<RecordId> {
        self.removed.iter().copied().collect()
    }

    /// Number of IDs on the removal list.
    pub fn removed_count(&self) -> usize {
        self.removed.len()
    }

    /// Number of records in the hold buffer.
    pub fn held_count(&self) -> usize {
        self.held.len()
    }

    /// Approximate wire bytes of an author's stored log.
    pub fn log_bytes(&self, author: &AuthorId) -> u64 {
        self.logs.get(author).map_or(0, |l| l.bytes)
    }

    /// Parses, verifies, and inserts one record line.
    pub fn insert_json(&mut self, line: &str) -> Result<Insert, RecordError> {
        Ok(self.insert(Record::from_json_str(line)?))
    }

    /// Inserts records in order. A record that is not stored on its own is
    /// tried as an evidence pair with the record before it, so a pair
    /// written as two consecutive lines is always recognized.
    pub fn insert_batch(&mut self, records: Vec<Record>) -> Vec<Insert> {
        let mut out = Vec::with_capacity(records.len());
        let mut prev: Option<Record> = None;
        for r in records {
            let o = self.insert_with_prev(prev.as_ref(), r.clone());
            out.push(o);
            prev = Some(r);
        }
        out
    }

    pub(crate) fn insert_with_prev(&mut self, prev: Option<&Record>, r: Record) -> Insert {
        let o = self.insert(r.clone());
        if matches!(o, Insert::Gap | Insert::AboveFork | Insert::Rejected(_))
            && let Some(p) = prev
            && fork_type(p, &r).is_ok()
        {
            return self.insert_evidence(p.clone(), r);
        }
        o
    }

    /// Parses JSON Lines (or a JSON array of records) and inserts every valid record.
    pub fn ingest(&mut self, text: &str) -> IngestReport {
        let mut rep = IngestReport::default();
        let trimmed = text.trim_start();
        if trimmed.starts_with('[') {
            match crate::parse_record_array(trimmed) {
                Ok(items) => {
                    let mut prev: Option<Record> = None;
                    for (i, item) in items.into_iter().enumerate() {
                        match item {
                            Ok(r) => {
                                let o = self.insert_with_prev(prev.as_ref(), r.clone());
                                rep.add(i + 1, Ok(o));
                                prev = Some(r);
                            }
                            Err(e) => {
                                rep.add(i + 1, Err(e));
                                prev = None;
                            }
                        }
                    }
                }
                Err(e) => rep.add(1, Err(e)),
            }
            return rep;
        }
        let mut prev: Option<Record> = None;
        for (line, res) in perch_core::parse_jsonl(text) {
            match res {
                Ok(r) => {
                    let o = self.insert_with_prev(prev.as_ref(), r.clone());
                    rep.add(line, Ok(o));
                    prev = Some(r);
                }
                Err(e) => {
                    rep.add(line, Err(e.to_string()));
                    prev = None;
                }
            }
        }
        rep
    }

    /// Every stored record in export order: per author (ascending), log
    /// records by seq, then the evidence pair `a`, `b`. Replaying this
    /// sequence with [`MemoryStore::insert_batch`] rebuilds the same state.
    pub fn export_records(&self) -> impl Iterator<Item = Record> + '_ {
        self.logs.iter().flat_map(move |(_, log)| {
            let recs = log.recs.values().map(move |id| self.entries[id].record.clone());
            let pair = log.pair.iter().flat_map(|p| [p.a().clone(), p.b().clone()]);
            recs.chain(pair)
        })
    }

    /// [`MemoryStore::export_records`] as JSON Lines.
    pub fn export_jsonl(&self) -> String {
        let mut s = String::new();
        for r in self.export_records() {
            s.push_str(&r.to_json_string());
            s.push('\n');
        }
        s
    }

    // ---- internals ----

    fn emit(&mut self, c: Change) {
        if self.track {
            self.changes.push(c);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn journal(&mut self, r: &Record) {
        if let Some(j) = self.jslot.file.as_mut()
            && let Err(e) = j.append(r)
            && self.jslot.error.is_none()
        {
            self.jslot.error = Some(e);
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn journal(&mut self, _r: &Record) {}

    fn record_ref(&self, id: &RecordId) -> Option<&Record> {
        if let Some(e) = self.entries.get(id) {
            return Some(&e.record);
        }
        let author = self.evidence_ids.get(id)?;
        let pair = self.logs.get(author)?.pair.as_ref()?;
        pair.records().into_iter().find(|r| r.id() == *id)
    }

    /// Root ID of a record's chain, following `replaces` through stored
    /// records of the same author, kind, and lower seq.
    fn root_of(&self, r: &Record) -> RecordId {
        let mut cur = r.clone();
        for _ in 0..1_000_000 {
            let Some(rep) = cur.replaces() else { break };
            match self.entries.get(&rep) {
                Some(e) if e.record.author() == cur.author() && e.record.seq() < cur.seq() && e.record.kind() == cur.kind() => {
                    cur = e.record.clone();
                }
                _ => break,
            }
        }
        cur.id()
    }

    fn compute_keys(&self, r: &Record) -> IndexKeys {
        let root = if r.kind() == names::PRODUCT { self.root_of(r) } else { r.id() };
        keys_for(r, root)
    }

    /// Stored records of `author` at seq − 1, seq, seq + 1 (log and pair).
    fn neighbors(&self, author: &AuthorId, seq: u64) -> Vec<Record> {
        let mut out = Vec::new();
        let Some(log) = self.logs.get(author) else { return out };
        let lo = seq.saturating_sub(1).max(1);
        for (_, id) in log.recs.range(lo..=seq.saturating_add(1)) {
            out.push(self.entries[id].record.clone());
        }
        if let Some(p) = &log.pair {
            for r in p.records() {
                if r.seq() + 1 >= seq && r.seq() <= seq.saturating_add(1) {
                    out.push(r.clone());
                }
            }
        }
        out
    }

    /// The lowest evidence pair `r` forms with a stored record, if any.
    fn best_candidate(&self, r: &Record) -> Option<EvidencePair> {
        let mut best: Option<EvidencePair> = None;
        for s in self.neighbors(&r.author(), r.seq()) {
            if let Ok(p) = EvidencePair::new(r.clone(), s) {
                best = Some(match best {
                    Some(b) => b.lowest(p),
                    None => p,
                });
            }
        }
        best
    }

    /// Stores `p` if it is lower than the author's pair. Truncates the log.
    fn offer_pair(&mut self, p: EvidencePair) -> bool {
        let author = p.author();
        let lower = match self.logs.get(&author).and_then(|l| l.pair.as_ref()) {
            Some(q) => p < *q,
            None => true,
        };
        if !lower {
            return false;
        }
        let p = match EvidencePair::new(p.a().without_body(), p.b().without_body()) {
            Ok(p) => p,
            Err(_) => return false,
        };
        let n = p.fork_seq();
        let log = self.logs.entry(author).or_default();
        if let Some(old) = log.pair.take() {
            for id in old.ids() {
                self.evidence_ids.remove(&id);
            }
        }
        let doomed: Vec<u64> = log.recs.range(n..).map(|(s, _)| *s).collect();
        for s in doomed {
            self.remove_log_record(&author, s);
        }
        for r in p.records() {
            self.evidence_ids.insert(r.id(), author);
        }
        if let Some(log) = self.logs.get_mut(&author) {
            log.pair = Some(p.clone());
        }
        // Held records above the fork seq can never be stored.
        let doomed: Vec<HoldKey> = self
            .held
            .range((author, n + 1, RecordId([0; 32]))..=(author, u64::MAX, RecordId([0xff; 32])))
            .map(|(k, _)| *k)
            .collect();
        for k in doomed {
            self.unhold(&k);
        }
        self.journal(&p.a().clone());
        self.journal(&p.b().clone());
        self.emit(Change::PairChanged(p));
        true
    }

    fn remove_log_record(&mut self, author: &AuthorId, seq: u64) {
        let Some(log) = self.logs.get_mut(author) else { return };
        let Some(id) = log.recs.remove(&seq) else { return };
        if let Some(e) = self.entries.remove(&id) {
            log.bytes = log.bytes.saturating_sub(wire_len(&e.record) as u64);
            if self.pool.contains_key(author) {
                self.pool_bytes = self.pool_bytes.saturating_sub(wire_len(&e.record) as u64);
            }
            self.idx.remove(&e.record, &e.keys);
            self.bodyless.remove(&id);
        }
    }

    fn admit(&mut self, r: &Record, count_after: u64) -> Result<(), Reject> {
        let Some(f) = self.admission.clone() else { return Ok(()) };
        let author = r.author();
        let lim = f(self, &author);
        let size = wire_len(r) as u64;
        let log_bytes = self.log_bytes(&author);
        if count_after > lim.records {
            return Err(Reject::Admission(format!("log limit of {} records for this author", lim.records)));
        }
        if log_bytes + size > lim.bytes {
            return Err(Reject::Admission(format!("log limit of {} bytes for this author", lim.bytes)));
        }
        let Some(rank) = lim.pool_rank else {
            self.pool_leave(&author);
            return Ok(());
        };
        let me = (rank, author);
        let full = |s: &MemoryStore| {
            Reject::Admission(format!("node budget of {} bytes for tier-3 authors is full", s.pool_budget))
        };
        if self.pool_cutoff.is_some_and(|c| me >= c) {
            return Err(full(self));
        }
        self.pool_join(author, rank);
        while self.pool_bytes + size > self.pool_budget {
            let worst = self.pool_order.iter().next_back().copied();
            match worst {
                Some(w) if w > me => {
                    self.remove_author(&w.1);
                    self.pool_cutoff = Some(self.pool_cutoff.map_or(w, |c| c.min(w)));
                }
                _ => {
                    if !self.logs.contains_key(&author) {
                        self.pool_leave(&author);
                    }
                    self.pool_cutoff = Some(self.pool_cutoff.map_or(me, |c| c.min(me)));
                    return Err(full(self));
                }
            }
        }
        Ok(())
    }

    fn pool_join(&mut self, author: AuthorId, rank: [u8; 32]) {
        match self.pool.get(&author) {
            Some(old) if *old == rank => {}
            Some(old) => {
                let old = *old;
                self.pool_order.remove(&(old, author));
                self.pool_order.insert((rank, author));
                self.pool.insert(author, rank);
            }
            None => {
                self.pool.insert(author, rank);
                self.pool_order.insert((rank, author));
                self.pool_bytes += self.log_bytes(&author);
            }
        }
    }

    fn pool_leave(&mut self, author: &AuthorId) {
        if let Some(rank) = self.pool.remove(author) {
            self.pool_order.remove(&(rank, *author));
            self.pool_bytes = self.pool_bytes.saturating_sub(self.log_bytes(author));
        }
    }

    /// Sets the byte budget of the shared pool (tier 3). Default: unlimited.
    pub fn set_pool_budget(&mut self, bytes: u64) {
        self.pool_budget = bytes;
    }

    /// Approximate wire bytes of the logs in the shared pool.
    pub fn pool_bytes(&self) -> u64 {
        self.pool_bytes
    }

    /// Authors in the shared pool.
    pub fn pool_len(&self) -> usize {
        self.pool.len()
    }

    /// True if the persistence file holds data the store no longer has
    /// (removed bodies or dropped logs); [`MemoryStore::compact`] rewrites it.
    pub fn needs_compaction(&self) -> bool {
        self.needs_compaction
    }

    /// Drops an author's whole log, evidence pair, and held records.
    /// Returns the number of log records dropped. Used by admission.
    pub fn remove_author(&mut self, author: &AuthorId) -> usize {
        self.pool_leave(author);
        self.ts_cut.remove(author);
        let seqs: Vec<u64> = self.logs.get(author).map(|l| l.recs.keys().copied().collect()).unwrap_or_default();
        let n = seqs.len();
        for s in seqs {
            self.remove_log_record(author, s);
        }
        if let Some(log) = self.logs.remove(author)
            && let Some(p) = log.pair
        {
            for id in p.ids() {
                self.evidence_ids.remove(&id);
            }
        }
        let held: Vec<HoldKey> = self
            .held
            .range((*author, 0, RecordId([0; 32]))..=(*author, u64::MAX, RecordId([0xff; 32])))
            .map(|(k, _)| *k)
            .collect();
        for k in held {
            self.unhold(&k);
        }
        self.needs_compaction = true;
        n
    }

    /// Recomputes pool membership: with `all`, for every author (for example
    /// after startup replay); otherwise only for current members (for
    /// example after the rank epoch changed). Then drops the highest ranked
    /// logs while the pool is over budget. Returns the dropped authors.
    pub fn rebuild_pool(&mut self, all: bool) -> Vec<AuthorId> {
        self.pool_cutoff = None;
        let Some(f) = self.admission.clone() else { return Vec::new() };
        let authors: Vec<AuthorId> = if all { self.logs.keys().copied().collect() } else { self.pool.keys().copied().collect() };
        for a in authors {
            match f(self, &a).pool_rank {
                Some(rank) => self.pool_join(a, rank),
                None => self.pool_leave(&a),
            }
        }
        let mut dropped = Vec::new();
        while self.pool_bytes > self.pool_budget {
            let Some(&w) = self.pool_order.iter().next_back() else { break };
            self.remove_author(&w.1);
            self.pool_cutoff = Some(self.pool_cutoff.map_or(w, |c| c.min(w)));
            dropped.push(w.1);
        }
        dropped
    }

    fn store_log_record(&mut self, r: Record) {
        let id = r.id();
        let author = r.author();
        let keys = self.compute_keys(&r);
        self.idx.add(&r, &keys);
        if !r.has_body() {
            self.bodyless.insert(id);
        }
        let log = self.logs.entry(author).or_default();
        log.recs.insert(r.seq(), id);
        log.bytes += wire_len(&r) as u64;
        if self.pool.contains_key(&author) {
            self.pool_bytes += wire_len(&r) as u64;
        }
        self.entries.insert(id, Entry { record: r.clone(), keys });
        if self.mode == Mode::Sparse && r.kind() == names::PRODUCT {
            self.reindex_descendants(id);
        }
        self.journal(&r);
        self.emit(Change::Inserted(r));
    }

    /// Recomputes index keys of `product` records that replace `id`, directly or transitively.
    fn reindex_descendants(&mut self, id: RecordId) {
        let mut stack: Vec<RecordId> = self.idx.replaced_by.get(&id).map(|s| s.iter().copied().collect()).unwrap_or_default();
        let mut seen = BTreeSet::new();
        while let Some(d) = stack.pop() {
            if !seen.insert(d) {
                continue;
            }
            self.reindex(&d);
            if let Some(s) = self.idx.replaced_by.get(&d) {
                stack.extend(s.iter().copied());
            }
        }
    }

    fn reindex(&mut self, id: &RecordId) {
        let Some(e) = self.entries.get(id).cloned() else { return };
        let keys = self.compute_keys(&e.record);
        if keys != e.keys {
            self.idx.remove(&e.record, &e.keys);
            self.idx.add(&e.record, &keys);
            if let Some(x) = self.entries.get_mut(id) {
                x.keys = keys;
            }
        }
    }

    fn replace_log_record(&mut self, new: Record) {
        let id = new.id();
        let Some(old) = self.entries.get(&id).cloned() else { return };
        let author = new.author();
        if let Some(log) = self.logs.get_mut(&author) {
            log.bytes = log.bytes.saturating_sub(wire_len(&old.record) as u64) + wire_len(&new) as u64;
        }
        if self.pool.contains_key(&author) {
            self.pool_bytes = self.pool_bytes.saturating_sub(wire_len(&old.record) as u64) + wire_len(&new) as u64;
        }
        self.idx.remove(&old.record, &old.keys);
        let keys = self.compute_keys(&new);
        self.idx.add(&new, &keys);
        if new.has_body() {
            self.bodyless.remove(&id);
        } else {
            self.bodyless.insert(id);
        }
        self.entries.insert(id, Entry { record: new, keys });
    }

    fn hold(&mut self, r: Record) {
        let key = (r.author(), r.seq(), r.id());
        if let Some((existing, _)) = self.held.get(&key) {
            if !existing.has_body() && r.has_body() {
                let c = self.held[&key].1;
                self.held_bytes = self.held_bytes + wire_len(&r) - wire_len(existing);
                self.held.insert(key, (r, c));
            }
            return;
        }
        let per = self.held_per_author.get(&key.0).copied().unwrap_or(0);
        if per >= self.hold_limits.per_author {
            return;
        }
        let size = wire_len(&r);
        while !self.held.is_empty()
            && (self.held.len() >= self.hold_limits.max_records || self.held_bytes + size > self.hold_limits.max_bytes)
        {
            let Some((_, k)) = self.held_order.iter().next().map(|(c, k)| (*c, *k)) else { break };
            self.unhold(&k);
        }
        self.counter += 1;
        self.held.insert(key, (r, self.counter));
        self.held_order.insert(self.counter, key);
        *self.held_per_author.entry(key.0).or_default() += 1;
        self.held_bytes += size;
    }

    fn unhold(&mut self, key: &HoldKey) -> Option<Record> {
        let (r, c) = self.held.remove(key)?;
        self.held_order.remove(&c);
        if let Some(n) = self.held_per_author.get_mut(&key.0) {
            *n -= 1;
            if *n == 0 {
                self.held_per_author.remove(&key.0);
            }
        }
        self.held_bytes = self.held_bytes.saturating_sub(wire_len(&r));
        Some(r)
    }

    fn head_seq(&self, author: &AuthorId) -> u64 {
        self.logs.get(author).and_then(|l| l.recs.keys().next_back().copied()).unwrap_or(0)
    }

    /// Applies held records that continue the log, until none do.
    fn drain(&mut self, author: AuthorId) {
        loop {
            let next = self.head_seq(&author) + 1;
            let keys: Vec<HoldKey> = self
                .held
                .range((author, next, RecordId([0; 32]))..=(author, next, RecordId([0xff; 32])))
                .map(|(k, _)| *k)
                .collect();
            if keys.is_empty() {
                return;
            }
            let mut progressed = false;
            for k in keys {
                if let Some(r) = self.unhold(&k) {
                    let o = self.insert_inner(r);
                    progressed |= o.changed();
                }
            }
            if !progressed {
                return;
            }
        }
    }

    /// Sparse mode: applies the log-level `ts` rule to the held records of
    /// `r`'s author plus `r`, in an order-independent way. The cut is the
    /// smallest seq `b` whose `ts` is below the highest `ts` at a lower held
    /// seq; records at or above it are dropped. Returns an outcome if `r` is not stored.
    fn sparse_ts_gate(&mut self, r: &Record) -> Option<Insert> {
        let author = r.author();
        let old = self.ts_cut.get(&author).copied();
        if old.is_some_and(|c| r.seq() >= c) {
            return Some(Insert::Gap);
        }
        let mut items: Vec<(u64, u64)> = match self.logs.get(&author) {
            Some(log) => log.recs.iter().map(|(s, id)| (*s, self.entries[id].record.ts())).collect(),
            None => Vec::new(),
        };
        items.push((r.seq(), r.ts()));
        items.sort();
        let mut max_ts = 0u64;
        let mut cut = None;
        let mut max_below_r = 0u64;
        for (seq, ts) in &items {
            if *seq < r.seq() {
                max_below_r = max_below_r.max(*ts);
            }
            if *ts < max_ts {
                cut = Some(*seq);
                break;
            }
            max_ts = max_ts.max(*ts);
        }
        let cut = cut?;
        let cut = old.map_or(cut, |c| c.min(cut));
        self.ts_cut.insert(author, cut);
        let doomed: Vec<u64> = self.logs.get(&author).map(|l| l.recs.range(cut..).map(|(s, _)| *s).collect()).unwrap_or_default();
        for s in doomed {
            self.remove_log_record(&author, s);
        }
        if r.seq() >= cut {
            if r.ts() < max_below_r {
                return Some(Insert::Rejected(Reject::TsDecrease { prev_ts: max_below_r, ts: r.ts() }));
            }
            return Some(Insert::Gap);
        }
        None
    }

    fn insert_inner(&mut self, mut r: Record) -> Insert {
        let id = r.id();
        if let Some(e) = self.entries.get(&id) {
            if !e.record.has_body() && r.has_body() && !self.removed.contains(&id) {
                self.replace_log_record(r.clone());
                self.journal(&r);
                self.emit(Change::BodyFilled(r));
                return Insert::BodyFilled;
            }
            return Insert::Duplicate;
        }
        if self.evidence_ids.contains_key(&id) {
            return Insert::Duplicate;
        }
        if r.has_body() && self.removed.contains(&id) && !kinds::is_control_kind(r.kind()) {
            r = r.without_body();
            self.needs_compaction = true;
        }
        if self.mode == Mode::Sparse
            && let Some(o) = self.sparse_ts_gate(&r)
        {
            return o;
        }
        if let Some(p) = self.best_candidate(&r) {
            let changed = self.offer_pair(p);
            return Insert::Fork { pair_changed: changed };
        }
        let author = r.author();
        let fork = self.logs.get(&author).and_then(|l| l.pair.as_ref()).map(|p| p.fork_seq());
        match self.mode {
            Mode::Sparse => {
                if fork.is_some_and(|n| r.seq() >= n) {
                    return Insert::AboveFork;
                }
                let count = self.logs.get(&author).map_or(0, |l| l.recs.len() as u64) + 1;
                if let Err(e) = self.admit(&r, count) {
                    return Insert::Rejected(e);
                }
                self.store_log_record(r);
                Insert::Inserted
            }
            Mode::Full => {
                let k = self.head_seq(&author);
                let cap = fork.map_or(u64::MAX, |n| n - 1);
                if r.seq() == k + 1 {
                    if r.seq() > cap {
                        return Insert::AboveFork;
                    }
                    if k >= 1
                        && let Some(p) = self.record_at(&author, k)
                        && r.ts() < p.ts()
                    {
                        return Insert::Rejected(Reject::TsDecrease { prev_ts: p.ts(), ts: r.ts() });
                    }
                    if let Err(e) = self.admit(&r, r.seq()) {
                        return Insert::Rejected(e);
                    }
                    self.store_log_record(r);
                    Insert::Inserted
                } else if r.seq() > k + 1 {
                    // Records up to the fork seq may still form a lower pair once the gap fills.
                    if r.seq() > cap.saturating_add(1) {
                        return Insert::AboveFork;
                    }
                    if let Some(f) = &self.admission
                        && r.seq() > f(self, &author).records
                    {
                        return Insert::Rejected(Reject::Admission("seq above the log limit for this author".into()));
                    }
                    self.hold(r);
                    Insert::Gap
                } else {
                    // A stored record exists at this seq with another ID, which
                    // best_candidate reports as a fork. Not reachable.
                    Insert::Duplicate
                }
            }
        }
    }
}

impl Store for MemoryStore {
    fn insert(&mut self, record: Record) -> Insert {
        let author = record.author();
        let o = self.insert_inner(record);
        if self.mode == Mode::Full && o.changed() {
            self.drain(author);
        }
        o
    }

    fn insert_evidence(&mut self, x: Record, y: Record) -> Insert {
        let p = match EvidencePair::new(x, y) {
            Ok(p) => p,
            Err(e) => return Insert::Rejected(Reject::NotEvidence(e)),
        };
        let mut best = p.clone();
        for r in p.records() {
            if let Some(c) = self.best_candidate(r) {
                best = best.lowest(c);
            }
        }
        let author = p.author();
        let changed = self.offer_pair(best);
        if changed && self.mode == Mode::Full {
            self.drain(author);
        }
        Insert::Fork { pair_changed: changed }
    }

    fn remove_body(&mut self, id: &RecordId) -> Result<bool, StoreError> {
        if let Some(r) = self.record_ref(id)
            && kinds::is_control_kind(r.kind())
        {
            return Err(StoreError::ControlKind(*id));
        }
        self.removed.insert(*id);
        let held_keys: Vec<HoldKey> = self.held.iter().filter(|(k, _)| k.2 == *id).map(|(k, _)| *k).collect();
        for k in held_keys {
            if let Some((r, _)) = self.held.get_mut(&k) {
                if kinds::is_control_kind(r.kind()) {
                    self.removed.remove(id);
                    return Err(StoreError::ControlKind(*id));
                }
                *r = r.without_body();
            }
        }
        let Some(e) = self.entries.get(id) else { return Ok(false) };
        if !e.record.has_body() {
            return Ok(false);
        }
        let stripped = e.record.without_body();
        self.replace_log_record(stripped);
        self.needs_compaction = true;
        #[cfg(not(target_arch = "wasm32"))]
        if self.jslot.file.is_some()
            && let Err(e) = self.compact()
        {
            return Err(StoreError::Io(e));
        }
        self.emit(Change::BodyRemoved(*id));
        Ok(true)
    }

    fn drain_changes(&mut self) -> Vec<Change> {
        std::mem::take(&mut self.changes)
    }

    fn get(&self, id: &RecordId) -> Option<Record> {
        self.record_ref(id).cloned()
    }

    fn contains(&self, id: &RecordId) -> bool {
        self.entries.contains_key(id) || self.evidence_ids.contains_key(id)
    }

    fn len(&self) -> usize {
        self.entries.len()
    }

    fn is_sparse(&self) -> bool {
        self.mode == Mode::Sparse
    }

    fn authors(&self) -> Vec<AuthorId> {
        self.logs.keys().copied().collect()
    }

    fn author_count(&self) -> usize {
        self.logs.len()
    }

    fn head(&self, author: &AuthorId) -> Option<Head> {
        let log = self.logs.get(author)?;
        let last = log.recs.iter().next_back();
        Some(Head {
            author: *author,
            seq: last.map_or(0, |(s, _)| *s),
            id: last.map(|(_, id)| *id),
            fork_seq: log.pair.as_ref().map(|p| p.fork_seq()),
            evidence: log.pair.as_ref().map(|p| p.ids()),
        })
    }

    fn heads(&self, after: Option<&AuthorId>, limit: usize) -> Vec<Head> {
        let lower = match after {
            Some(a) => Bound::Excluded(*a),
            None => Bound::Unbounded,
        };
        self.logs
            .range((lower, Bound::Unbounded))
            .take(limit)
            .filter_map(|(a, _)| self.head(a))
            .collect()
    }

    fn log(&self, author: &AuthorId, from_seq: u64, limit: usize) -> Vec<Record> {
        let Some(log) = self.logs.get(author) else { return Vec::new() };
        log.recs.range(from_seq..).take(limit).map(|(_, id)| self.entries[id].record.clone()).collect()
    }

    fn record_at(&self, author: &AuthorId, seq: u64) -> Option<Record> {
        let id = self.logs.get(author)?.recs.get(&seq)?;
        Some(self.entries[id].record.clone())
    }

    fn fork_seq(&self, author: &AuthorId) -> Option<u64> {
        self.logs.get(author)?.pair.as_ref().map(|p| p.fork_seq())
    }

    fn evidence(&self, author: &AuthorId) -> Option<EvidencePair> {
        self.logs.get(author)?.pair.clone()
    }

    fn forked_authors(&self) -> Vec<AuthorId> {
        self.logs.iter().filter(|(_, l)| l.pair.is_some()).map(|(a, _)| *a).collect()
    }

    fn by_kind(&self, kind: &str) -> Vec<RecordId> {
        self.idx.by_kind.get(kind).map(|s| s.iter().copied().collect()).unwrap_or_default()
    }

    fn by_author_kind(&self, author: &AuthorId, kind: &str) -> Vec<RecordId> {
        self.idx
            .by_author_kind
            .get(&(*author, kind.to_string()))
            .map(|m| m.values().copied().collect())
            .unwrap_or_default()
    }

    fn by_product(&self, key: &str) -> Vec<RecordId> {
        self.idx.by_product.get(key).map(|s| s.iter().copied().collect()).unwrap_or_default()
    }

    fn product_keys(&self) -> Vec<String> {
        self.idx.by_product.keys().cloned().collect()
    }

    fn product_key_count(&self) -> usize {
        self.idx.by_product.len()
    }

    fn by_target(&self, id: &RecordId) -> Vec<RecordId> {
        self.idx.by_target.get(id).map(|s| s.iter().copied().collect()).unwrap_or_default()
    }

    fn referencing(&self, id: &RecordId) -> Vec<RecordId> {
        self.idx.referencing.get(id).map(|s| s.iter().copied().collect()).unwrap_or_default()
    }

    fn replaced_by(&self, id: &RecordId) -> Vec<RecordId> {
        self.idx.replaced_by.get(id).map(|s| s.iter().copied().collect()).unwrap_or_default()
    }

    fn by_subject(&self, author: &AuthorId) -> Vec<RecordId> {
        self.idx.by_subject.get(author).map(|s| s.iter().copied().collect()).unwrap_or_default()
    }

    fn by_person(&self, attestor: &AuthorId, person: &Hex32) -> Vec<RecordId> {
        self.idx.by_person.get(&(*attestor, *person)).map(|s| s.iter().copied().collect()).unwrap_or_default()
    }

    fn missing_bodies(&self, after: Option<&RecordId>, limit: usize) -> Vec<RecordId> {
        let lower = match after {
            Some(a) => Bound::Excluded(*a),
            None => Bound::Unbounded,
        };
        self.bodyless
            .range((lower, Bound::Unbounded))
            .filter(|id| !self.removed.contains(id))
            .take(limit)
            .copied()
            .collect()
    }

    fn bodyless_count(&self) -> usize {
        self.bodyless.len()
    }

    fn is_removed(&self, id: &RecordId) -> bool {
        self.removed.contains(id)
    }
}
