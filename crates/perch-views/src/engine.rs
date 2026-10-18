//! The view engine: record pipeline, validity, chains, identity, persons,
//! flags, and restriction (docs/views.md sections 3 to 7).
//!
//! [`Views`] memoizes every derived value for one (store, policy) pair.
//! All store access goes through accessor methods, which can record every
//! record ID they return (the related-set trace, section 14).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Arc;

use perch_core::kinds::{self, Body, Problem, names};
use perch_core::{AuthorId, Hex32, ProductKey, Record, RecordId};
use perch_store::Store;

use crate::model::{Appeal, AuthorStatus, Flags, Moderation, Redaction, Status};
use crate::policy::Policy;

// ---------------------------------------------------------------------------
// Fast deterministic hashing for lookup maps (never iterated for output).
// ---------------------------------------------------------------------------

#[derive(Default, Clone, Copy)]
pub(crate) struct FxHasher(u64);

impl Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut b = [0u8; 8];
            b[..chunk.len()].copy_from_slice(chunk);
            self.0 = (self.0.rotate_left(5) ^ u64::from_le_bytes(b)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

pub(crate) type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

const NONE: u64 = u64::MAX;

/// Kinds whose chains have a display status that flags can change.
pub(crate) const HIDEABLE: [&str; 4] = [names::LISTING, names::REVIEW, names::PRODUCT, names::PROFILE];

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// A stored record with its typed body when the schema check passes.
pub(crate) struct Rec {
    pub r: Record,
    /// `(typed body, schema failure reason)`, parsed on first use.
    parsed: std::cell::OnceCell<(Option<Body>, Option<String>)>,
}

impl Rec {
    fn new(r: Record) -> Rec {
        Rec { r, parsed: std::cell::OnceCell::new() }
    }
    fn parsed(&self) -> &(Option<Body>, Option<String>) {
        self.parsed.get_or_init(|| {
            if !self.r.has_body() || !kinds::is_known_kind(self.r.kind()) {
                (None, None)
            } else {
                match kinds::check_record(&self.r) {
                    Ok(b) => (Some(b), None),
                    Err(e) => (None, Some(schema_reason(&e))),
                }
            }
        })
    }
    /// Typed body; `None` when the body is absent or fails the schema.
    pub fn body(&self) -> Option<&Body> {
        self.parsed().0.as_ref()
    }
    /// Schema failure reason, when the body is present and fails.
    pub fn schema_err(&self) -> Option<&String> {
        self.parsed().1.as_ref()
    }
    pub fn id(&self) -> RecordId {
        self.r.id()
    }
    pub fn author(&self) -> AuthorId {
        self.r.author()
    }
    pub fn seq(&self) -> u64 {
        self.r.seq()
    }
    pub fn ts(&self) -> u64 {
        self.r.ts()
    }
    pub fn kind(&self) -> &str {
        self.r.kind()
    }
    pub fn has_body(&self) -> bool {
        self.r.has_body()
    }
}

fn schema_reason(e: &kinds::SchemaError) -> String {
    match e.problem {
        Problem::NotReplaceable => "replaces:not_replaceable".to_owned(),
        _ => format!("schema:{}", e.field),
    }
}

/// Semantic validity (docs/views.md section 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Validity {
    Valid,
    Invalid(String),
    Unavailable,
    Unknown,
}

impl Validity {
    pub fn is_valid(&self) -> bool {
        matches!(self, Validity::Valid)
    }
}

/// Policy-independent part of a chain.
pub(crate) struct ChainBase {
    pub root: RecordId,
    pub author: AuthorId,
    pub kind: String,
    /// Members ascending by seq.
    pub members: Vec<Arc<Rec>>,
    /// Lowest-seq member with a schema-valid body.
    pub reference: Option<Arc<Rec>>,
}

/// A chain under the policy.
pub(crate) struct Chain {
    pub base: Arc<ChainBase>,
    /// Counting retract with the lowest seq.
    pub retract: Option<Arc<Rec>>,
    /// Highest-seq candidate.
    pub latest: Option<Arc<Rec>>,
    /// Seq cap for candidates (`not_after_seq` for attestation chains).
    cap: u64,
}

impl Chain {
    pub fn root(&self) -> RecordId {
        self.base.root
    }
    pub fn author(&self) -> AuthorId {
        self.base.author
    }
    pub fn kind(&self) -> &str {
        &self.base.kind
    }
    pub fn reference(&self) -> Option<&Arc<Rec>> {
        self.base.reference.as_ref()
    }
    /// The latest version's typed body, if any.
    pub fn latest_body(&self) -> Option<&Body> {
        self.latest.as_ref().and_then(|l| l.body())
    }
}

/// Product key from a chain's reference version.
pub(crate) fn reference_product(chain: &ChainBase) -> Option<&ProductKey> {
    chain.reference.as_ref().and_then(|r| r.body()).and_then(|b| b.product())
}

/// Per-author cutoffs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PreCuts {
    pub fork_seq: u64,
    pub retire_a: u64,
}

#[derive(Clone)]
pub(crate) struct Cuts {
    pub pre: PreCuts,
    pub revoke_cut: u64,
    /// `(valid_through_seq, attestor, revoke id)` of the reported revoke.
    pub revoke: Option<(u64, AuthorId, RecordId)>,
    pub retire_seq: u64,
    pub retire: Option<Arc<Rec>>,
}

impl Cuts {
    pub fn status(&self) -> AuthorStatus {
        if self.revoke_cut != NONE {
            AuthorStatus::Revoked
        } else if self.pre.fork_seq != NONE {
            AuthorStatus::Forked
        } else if self.retire_seq != NONE {
            AuthorStatus::Retired
        } else {
            AuthorStatus::Ok
        }
    }
    /// Fork evidence and no counting revoke.
    pub fn excluded_forked(&self) -> bool {
        self.pre.fork_seq != NONE && self.revoke_cut == NONE
    }
}

/// A counting `attest.identity`.
#[derive(Debug, Clone)]
pub(crate) struct IdAtt {
    pub attestor: AuthorId,
    pub person: Hex32,
    pub level: &'static str,
    pub legal_name: Option<String>,
    pub country: Option<String>,
    pub expires: u64,
    pub root: RecordId,
}

/// A counting `attest.domain`.
#[derive(Debug, Clone)]
pub(crate) struct DomAtt {
    pub domain: String,
    pub attestor: AuthorId,
    pub expires: u64,
    pub root: RecordId,
}

pub(crate) struct Ident {
    /// Sorted by `(attestor, person, root)`.
    pub ids: Vec<IdAtt>,
    /// Sorted by `(domain, attestor, root)`.
    pub domains: Vec<DomAtt>,
}

/// A person: a connected component of keys (7.3).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum PersonId {
    Pair(AuthorId, Hex32),
    Key(AuthorId),
}

pub(crate) struct Person {
    pub id: PersonId,
    /// Keys, ascending.
    pub keys: Vec<AuthorId>,
}

pub(crate) struct Restriction {
    pub hidden_listings: u64,
    pub distinct_flaggers: u64,
    pub restricted: bool,
}

/// State of one flag chain on a target.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlagState {
    Retracted,
    Unavailable,
    Live,
}

pub(crate) struct FlagEntry {
    pub chain: Arc<Chain>,
    pub state: FlagState,
    pub excluded: Option<&'static str>,
}

pub(crate) struct FlagInfo {
    /// Sorted by `(latest ts, latest id)`.
    pub entries: Vec<FlagEntry>,
    pub flags: Flags,
    pub counted_persons: BTreeSet<PersonId>,
}

/// Counting outcome of one review chain.
pub(crate) struct ReviewEval {
    pub chain: Arc<Chain>,
    pub excluded: Option<&'static str>,
}

pub(crate) struct ReviewSet {
    /// Review chains with a latest version, sorted by root.
    pub evals: Vec<ReviewEval>,
    /// Chains ignored because every member with a body is above a revoke cut.
    pub revoked: u64,
}

/// Verified-purchase outcome.
#[derive(Clone, Copy)]
pub(crate) struct Purchase {
    pub outcome: &'static str,
    /// Seller of a valid receipt (signature, root, and buyer checks passed).
    pub receipt_seller: Option<AuthorId>,
}

// ---------------------------------------------------------------------------
// The engine
// ---------------------------------------------------------------------------

/// Views over one store under one policy, with memoized intermediate
/// results. Create one per query batch; results never depend on the order
/// of queries.
pub struct Views<'a, S: Store + ?Sized> {
    pub(crate) store: &'a S,
    pub(crate) policy: &'a Policy,
    trace: Option<RefCell<BTreeSet<RecordId>>>,
    c: Caches,
}

/// Memoized intermediate results of a [`Views`] context.
#[derive(Default)]
pub(crate) struct Caches {
    recs: RefCell<FastMap<RecordId, Option<Arc<Rec>>>>,
    roots: RefCell<FastMap<RecordId, RecordId>>,
    bases: RefCell<FastMap<RecordId, Arc<ChainBase>>>,
    chains: RefCell<FastMap<RecordId, Arc<Chain>>>,
    validity: RefCell<FastMap<RecordId, Validity>>,
    pre_cuts: RefCell<FastMap<AuthorId, PreCuts>>,
    cuts: RefCell<FastMap<AuthorId, Arc<Cuts>>>,
    idents: RefCell<FastMap<AuthorId, Arc<Ident>>>,
    persons: RefCell<FastMap<AuthorId, Arc<Person>>>,
    restrictions: RefCell<FastMap<PersonId, Arc<Restriction>>>,
    flag_infos: RefCell<FastMap<RecordId, Arc<FlagInfo>>>,
    by_product_cache: RefCell<FastMap<String, Arc<ProductRecords>>>,
    seller_persons: RefCell<FastMap<String, Arc<BTreeSet<PersonId>>>>,
    review_sets: RefCell<FastMap<String, Arc<ReviewSet>>>,
    purchases: RefCell<FastMap<RecordId, Purchase>>,
    profiles: RefCell<FastMap<AuthorId, Option<Arc<Chain>>>>,
    hosts: RefCell<FastMap<RecordId, Option<String>>>,
}

/// What a [`ViewCache`] was computed for.
#[derive(Clone, PartialEq, Eq)]
struct CacheKey {
    policy: Policy,
    len: usize,
    bodyless: usize,
    forked: Vec<(AuthorId, Option<[RecordId; 2]>)>,
}

/// Memoized results kept between queries, for callers that run many views
/// over an unchanged store (the node, the wasm client).
///
/// A cache is valid only for the store contents and policy it was computed
/// for. Callers must discard it after any store change (insert, body fill,
/// body removal) or policy change. As a safety net, [`Views::with_cache`]
/// discards a cache whose policy, record count, body-less count, or fork
/// evidence differ. Results with and without a cache are identical.
#[derive(Default)]
pub struct ViewCache {
    caches: Caches,
    key: Option<CacheKey>,
}

impl ViewCache {
    /// An empty cache.
    pub fn new() -> ViewCache {
        ViewCache::default()
    }
}

fn cache_key<S: Store + ?Sized>(store: &S, policy: &Policy) -> CacheKey {
    CacheKey {
        policy: policy.clone(),
        len: store.len(),
        bodyless: store.bodyless_count(),
        forked: store.forked_authors().into_iter().map(|a| (a, store.evidence(&a).map(|p| p.ids()))).collect(),
    }
}

/// Chains of one product key, by kind, sorted by root.
pub(crate) struct ProductRecords {
    pub products: Vec<Arc<Chain>>,
    pub listings: Vec<Arc<Chain>>,
    pub reviews: Vec<Arc<Chain>>,
}

impl<'a, S: Store + ?Sized> Views<'a, S> {
    /// A view context for `store` under `policy`.
    pub fn new(store: &'a S, policy: &'a Policy) -> Self {
        Views { store, policy, trace: None, c: Caches::default() }
    }

    /// A view context that reuses `cache` if it was computed for the same
    /// store contents and policy (see [`ViewCache`]).
    pub fn with_cache(store: &'a S, policy: &'a Policy, cache: ViewCache) -> Self {
        let key = cache_key(store, policy);
        let c = if cache.key.as_ref() == Some(&key) { cache.caches } else { Caches::default() };
        Views { store, policy, trace: None, c }
    }

    /// The memoized results, for reuse with [`Views::with_cache`].
    pub fn into_cache(self) -> ViewCache {
        let key = cache_key(self.store, self.policy);
        ViewCache { caches: self.c, key: Some(key) }
    }

    /// A context that records every record ID it reads.
    pub(crate) fn traced(store: &'a S, policy: &'a Policy) -> Self {
        let mut v = Self::new(store, policy);
        v.trace = Some(RefCell::new(BTreeSet::new()));
        v
    }

    pub(crate) fn take_trace(&self) -> Vec<RecordId> {
        self.trace.as_ref().map(|t| t.borrow().iter().copied().collect()).unwrap_or_default()
    }

    /// The policy.
    pub fn policy(&self) -> &Policy {
        self.policy
    }

    // --- Store accessors (traced) ----------------------------------------

    fn note(&self, ids: &[RecordId]) {
        if let Some(t) = &self.trace {
            t.borrow_mut().extend(ids.iter().copied());
        }
    }

    pub(crate) fn rec(&self, id: &RecordId) -> Option<Arc<Rec>> {
        if let Some(r) = self.c.recs.borrow().get(id) {
            return r.clone();
        }
        let r = self.store.get(id).map(|r| Arc::new(Rec::new(r)));
        if r.is_some() {
            self.note(&[*id]);
        }
        self.c.recs.borrow_mut().insert(*id, r.clone());
        r
    }

    fn recs_of(&self, ids: Vec<RecordId>) -> Vec<Arc<Rec>> {
        self.note(&ids);
        ids.iter().filter_map(|id| self.rec(id)).collect()
    }

    pub(crate) fn s_by_target(&self, id: &RecordId) -> Vec<Arc<Rec>> {
        self.recs_of(self.store.by_target(id))
    }
    pub(crate) fn s_referencing(&self, id: &RecordId) -> Vec<Arc<Rec>> {
        self.recs_of(self.store.referencing(id))
    }
    pub(crate) fn s_by_subject(&self, a: &AuthorId) -> Vec<Arc<Rec>> {
        self.recs_of(self.store.by_subject(a))
    }
    pub(crate) fn s_by_author_kind(&self, a: &AuthorId, kind: &str) -> Vec<Arc<Rec>> {
        self.recs_of(self.store.by_author_kind(a, kind))
    }
    fn s_replaced_by(&self, id: &RecordId) -> Vec<Arc<Rec>> {
        self.recs_of(self.store.replaced_by(id))
    }
    fn s_by_person(&self, a: &AuthorId, p: &Hex32) -> Vec<Arc<Rec>> {
        self.recs_of(self.store.by_person(a, p))
    }
    pub(crate) fn s_by_product(&self, key: &str) -> Vec<Arc<Rec>> {
        self.recs_of(self.store.by_product(key))
    }
    pub(crate) fn s_fork_seq(&self, a: &AuthorId) -> Option<u64> {
        let f = self.store.fork_seq(a);
        if f.is_some()
            && self.trace.is_some()
            && let Some(p) = self.store.evidence(a)
        {
            self.note(&p.ids());
        }
        f
    }

    // --- Time and activity -------------------------------------------------

    pub(crate) fn future(&self, r: &Rec) -> bool {
        self.policy.is_future(r.ts())
    }

    pub(crate) fn pre_cuts(&self, a: &AuthorId) -> PreCuts {
        if let Some(c) = self.c.pre_cuts.borrow().get(a) {
            return *c;
        }
        let fork_seq = self.s_fork_seq(a).unwrap_or(NONE);
        let mut retire_a = NONE;
        let mut rs = self.s_by_author_kind(a, names::RETIRE);
        rs.sort_by_key(|r| r.seq());
        for r in rs {
            if r.seq() < fork_seq && !self.future(&r) && self.validity(&r.id()).is_valid() {
                retire_a = r.seq();
                break;
            }
        }
        let c = PreCuts { fork_seq, retire_a };
        self.c.pre_cuts.borrow_mut().insert(*a, c);
        c
    }

    pub(crate) fn pre_active(&self, r: &Rec) -> bool {
        let c = self.pre_cuts(&r.author());
        !self.future(r) && r.seq() < c.fork_seq && r.seq() <= c.retire_a
    }

    pub(crate) fn cuts(&self, a: &AuthorId) -> Arc<Cuts> {
        if let Some(c) = self.c.cuts.borrow().get(a) {
            return c.clone();
        }
        let pre = self.pre_cuts(a);
        // Revokes (7.3): counted with the revoking attestor's pre-activity only.
        let subject_recs = self.s_by_subject(a);
        let mut revoke: Option<(u64, AuthorId, RecordId)> = None;
        for v in subject_recs.iter().filter(|r| r.kind() == names::ATTEST_REVOKE) {
            let att = v.author();
            if !self.policy.trusts(&att, names::ATTEST_REVOKE) || v.seq() > self.policy.cap(&att) {
                continue;
            }
            if !self.pre_active(v) || !self.validity(&v.id()).is_valid() {
                continue;
            }
            let issued = subject_recs.iter().any(|x| {
                x.author() == att
                    && (x.kind() == names::ATTEST_IDENTITY || x.kind() == names::ATTEST_DOMAIN)
                    && x.seq() < v.seq()
                    && self.validity(&x.id()).is_valid()
            });
            if !issued {
                continue;
            }
            let Some(Body::AttestRevoke(b)) = &v.body() else { continue };
            let cand = (b.valid_through_seq, att, v.id());
            if revoke.as_ref().is_none_or(|r| cand < *r) {
                revoke = Some(cand);
            }
        }
        let revoke_cut = revoke.map_or(NONE, |r| r.0);
        let mut retire = None;
        let mut retire_seq = NONE;
        let mut rs = self.s_by_author_kind(a, names::RETIRE);
        rs.sort_by_key(|r| r.seq());
        for r in rs {
            if r.seq() < pre.fork_seq && r.seq() <= revoke_cut && !self.future(&r) && self.validity(&r.id()).is_valid() {
                retire_seq = r.seq();
                retire = Some(r);
                break;
            }
        }
        let c = Arc::new(Cuts { pre, revoke_cut, revoke, retire_seq, retire });
        self.c.cuts.borrow_mut().insert(*a, c.clone());
        c
    }

    /// Active (docs/views.md section 3.2).
    pub(crate) fn active(&self, r: &Rec) -> bool {
        if self.future(r) {
            return false;
        }
        let c = self.cuts(&r.author());
        r.seq() < c.pre.fork_seq && r.seq() <= c.revoke_cut && r.seq() <= c.retire_seq
    }

    /// Why an otherwise valid record is ignored.
    pub(crate) fn ignored_reason(&self, r: &Rec) -> Option<&'static str> {
        if self.future(r) {
            return Some("future");
        }
        let c = self.cuts(&r.author());
        if r.seq() >= c.pre.fork_seq {
            Some("forked")
        } else if r.seq() > c.revoke_cut {
            Some("revoked")
        } else if r.seq() > c.retire_seq {
            Some("retired")
        } else {
            None
        }
    }

    // --- Chains -------------------------------------------------------------

    fn link_ok(&self, child: &Rec, parent: &Rec) -> bool {
        parent.author() == child.author()
            && parent.seq() < child.seq()
            && parent.kind() == child.kind()
            && kinds::is_replaceable(child.kind())
    }

    /// Root ID of the chain of `id` (section 3.4). Unknown IDs are their own root.
    pub(crate) fn root_of(&self, id: &RecordId) -> RecordId {
        if let Some(r) = self.c.roots.borrow().get(id) {
            return *r;
        }
        let root = match self.rec(id) {
            None => *id,
            Some(r) => match r.r.replaces() {
                None => *id,
                Some(p) => match self.rec(&p) {
                    Some(parent) if self.link_ok(&r, &parent) => self.root_of(&p),
                    _ => *id,
                },
            },
        };
        self.c.roots.borrow_mut().insert(*id, root);
        root
    }

    pub(crate) fn chain_base(&self, root: &RecordId) -> Option<Arc<ChainBase>> {
        if let Some(b) = self.c.bases.borrow().get(root) {
            return Some(b.clone());
        }
        let r = self.rec(root)?;
        let mut members = vec![r.clone()];
        let mut stack = vec![r.clone()];
        while let Some(x) = stack.pop() {
            for c in self.s_replaced_by(&x.id()) {
                if c.r.replaces() == Some(x.id()) && self.link_ok(&c, &x) {
                    members.push(c.clone());
                    stack.push(c);
                }
            }
        }
        members.sort_by_key(|m| (m.seq(), m.id()));
        let reference = members.iter().find(|m| m.body().is_some()).cloned();
        let b = Arc::new(ChainBase { root: *root, author: r.author(), kind: r.kind().to_owned(), members, reference });
        self.c.bases.borrow_mut().insert(*root, b.clone());
        Some(b)
    }

    /// The chain with root `root` under the policy.
    pub(crate) fn chain(&self, root: &RecordId) -> Option<Arc<Chain>> {
        if let Some(c) = self.c.chains.borrow().get(root) {
            return Some(c.clone());
        }
        let base = self.chain_base(root)?;
        let cap = if base.kind.starts_with("attest.") { self.policy.cap(&base.author) } else { NONE };
        let mut retract: Option<Arc<Rec>> = None;
        if kinds::is_retractable(&base.kind) {
            for m in &base.members {
                for t in self.s_by_target(&m.id()) {
                    if t.kind() == names::RETRACT
                        && t.author() == base.author
                        && t.seq() <= cap
                        && self.validity(&t.id()).is_valid()
                        && self.active(&t)
                        && retract.as_ref().is_none_or(|x| t.seq() < x.seq())
                    {
                        retract = Some(t);
                    }
                }
            }
        }
        // Latest version: scan from the highest seq, so older versions are
        // validated only when [`Views::versions`] needs them.
        let latest = base.members.iter().rev().find(|m| self.is_candidate(&base, m, retract.as_ref(), cap)).cloned();
        let c = Arc::new(Chain { base, retract, latest, cap });
        self.c.chains.borrow_mut().insert(*root, c.clone());
        Some(c)
    }

    fn is_candidate(&self, base: &ChainBase, m: &Rec, retract: Option<&Arc<Rec>>, cap: u64) -> bool {
        m.seq() < retract.map_or(NONE, |t| t.seq())
            && m.seq() <= cap
            && self.active(m)
            && if m.has_body() { self.validity(&m.id()).is_valid() } else { !kinds::is_control_kind(&base.kind) }
    }

    /// Candidate versions of a chain, ascending by seq (3.5).
    pub(crate) fn versions(&self, c: &Chain) -> Vec<Arc<Rec>> {
        c.base.members.iter().filter(|m| self.is_candidate(&c.base, m, c.retract.as_ref(), c.cap)).cloned().collect()
    }

    /// The chain containing record `id`.
    pub(crate) fn chain_of(&self, id: &RecordId) -> Option<Arc<Chain>> {
        let root = self.root_of(id);
        self.chain(&root)
    }

    // --- Validity -------------------------------------------------------------

    /// Semantic validity (section 4). Independent of the policy.
    pub(crate) fn validity(&self, id: &RecordId) -> Validity {
        if let Some(v) = self.c.validity.borrow().get(id) {
            return v.clone();
        }
        let v = self.compute_validity(id);
        self.c.validity.borrow_mut().insert(*id, v.clone());
        v
    }

    fn compute_validity(&self, id: &RecordId) -> Validity {
        let Some(r) = self.rec(id) else { return Validity::Unknown };
        if !kinds::is_known_kind(r.kind()) {
            return Validity::Unknown;
        }
        if !r.has_body() {
            return Validity::Unavailable;
        }
        if let Some(e) = r.schema_err() {
            return Validity::Invalid(e.clone());
        }
        let inv = |s: &str| Validity::Invalid(s.to_owned());
        if let Some(p) = r.r.replaces() {
            let Some(parent) = self.rec(&p) else { return inv("replaces:not_found") };
            if parent.author() != r.author() {
                return inv("replaces:author");
            }
            if parent.seq() >= r.seq() {
                return inv("replaces:seq");
            }
            if parent.kind() != r.kind() {
                return inv("replaces:kind");
            }
            if !kinds::is_replaceable(r.kind()) {
                return inv("replaces:not_replaceable");
            }
            let root = self.root_of(id);
            if let Some(base) = self.chain_base(&root)
                && let Some(reference) = &base.reference
                && reference.id() != *id
                && let (Some(old), Some(new)) = (reference.r.body(), r.r.body())
                && let Err(e) = kinds::check_fixed_fields(r.kind(), old, new)
            {
                return Validity::Invalid(format!("fixed:{}", e.field));
            }
        }
        match &r.body() {
            Some(Body::Retract(b)) => {
                if let Some(t) = self.rec(&b.target) {
                    if t.author() != r.author() {
                        return inv("retract:author");
                    }
                    if t.seq() >= r.seq() {
                        return inv("retract:seq");
                    }
                    if !kinds::is_retractable(t.kind()) {
                        return inv("retract:target_kind");
                    }
                }
            }
            Some(Body::Appeal(b)) => {
                if let Some(t) = self.rec(&b.target)
                    && t.author() != r.author()
                {
                    return inv("appeal:author");
                }
            }
            Some(Body::Review(b)) => {
                let refs = b.listing.iter().chain(b.receipt.as_ref().map(|rc| &rc.listing));
                for l in refs {
                    if let Some(x) = self.rec(l)
                        && x.kind() != names::LISTING
                    {
                        return inv("ref:listing_kind");
                    }
                }
            }
            Some(Body::Observation(b)) => {
                if let Some(l) = &b.listing
                    && let Some(x) = self.rec(l)
                    && x.kind() != names::LISTING
                {
                    return inv("ref:listing_kind");
                }
            }
            _ => {}
        }
        Validity::Valid
    }

    // --- Identity ---------------------------------------------------------------

    pub(crate) fn ident(&self, a: &AuthorId) -> Arc<Ident> {
        if let Some(i) = self.c.idents.borrow().get(a) {
            return i.clone();
        }
        let mut roots = BTreeSet::new();
        for r in self.s_by_subject(a) {
            let k = r.kind();
            if (k == names::ATTEST_IDENTITY || k == names::ATTEST_DOMAIN) && self.policy.trusts(&r.author(), k) {
                roots.insert(self.root_of(&r.id()));
            }
        }
        let mut ids = Vec::new();
        let mut domains = Vec::new();
        for root in roots {
            let Some(c) = self.chain(&root) else { continue };
            if c.retract.is_some() {
                continue;
            }
            match c.latest_body() {
                Some(Body::AttestIdentity(b)) if b.subject == *a && b.expires > self.policy.now => ids.push(IdAtt {
                    attestor: c.author(),
                    person: b.person,
                    level: b.level.as_str(),
                    legal_name: b.legal_name.clone(),
                    country: b.country.clone(),
                    expires: b.expires,
                    root,
                }),
                Some(Body::AttestDomain(b)) if b.subject == *a && b.expires > self.policy.now => domains.push(DomAtt {
                    domain: b.domain.clone(),
                    attestor: c.author(),
                    expires: b.expires,
                    root,
                }),
                _ => {}
            }
        }
        ids.sort_by(|x, y| (x.attestor, x.person, x.root).cmp(&(y.attestor, y.person, y.root)));
        domains.sort_by(|x, y| (&x.domain, x.attestor, x.root).cmp(&(&y.domain, y.attestor, y.root)));
        let i = Arc::new(Ident { ids, domains });
        self.c.idents.borrow_mut().insert(*a, i.clone());
        i
    }

    /// Verified (7.3).
    pub(crate) fn verified(&self, a: &AuthorId) -> bool {
        !self.ident(a).ids.is_empty()
    }

    fn has_pair(&self, a: &AuthorId, pair: &(AuthorId, Hex32)) -> bool {
        self.ident(a).ids.iter().any(|i| (i.attestor, i.person) == *pair)
    }

    /// The person of `a` (7.3).
    pub(crate) fn person(&self, a: &AuthorId) -> Arc<Person> {
        if let Some(p) = self.c.persons.borrow().get(a) {
            return p.clone();
        }
        let mut keys = BTreeSet::from([*a]);
        let mut pairs = BTreeSet::new();
        let mut queue = vec![*a];
        while let Some(k) = queue.pop() {
            let id = self.ident(&k);
            for att in &id.ids {
                let pair = (att.attestor, att.person);
                if !pairs.insert(pair) {
                    continue;
                }
                for r in self.s_by_person(&att.attestor, &att.person) {
                    let Some(Body::AttestIdentity(b)) = &r.body() else { continue };
                    if !keys.contains(&b.subject) && self.has_pair(&b.subject, &pair) {
                        keys.insert(b.subject);
                        queue.push(b.subject);
                    }
                }
            }
        }
        let id = match pairs.iter().next() {
            Some((a, p)) => PersonId::Pair(*a, *p),
            None => PersonId::Key(*a),
        };
        let p = Arc::new(Person { id, keys: keys.into_iter().collect() });
        let mut m = self.c.persons.borrow_mut();
        for k in &p.keys {
            m.insert(*k, p.clone());
        }
        p
    }

    pub(crate) fn person_id(&self, a: &AuthorId) -> PersonId {
        self.person(a).id.clone()
    }

    pub(crate) fn excluded_forked(&self, a: &AuthorId) -> bool {
        self.cuts(a).excluded_forked()
    }

    pub(crate) fn status(&self, a: &AuthorId) -> AuthorStatus {
        self.cuts(a).status()
    }

    // --- Products -----------------------------------------------------------------

    /// Chains for a product key (sections 9.1, 10.2).
    pub(crate) fn product_records(&self, key: &str) -> Arc<ProductRecords> {
        if let Some(p) = self.c.by_product_cache.borrow().get(key) {
            return p.clone();
        }
        let recs = self.s_by_product(key);
        let mut roots: BTreeMap<(&'static str, RecordId), ()> = BTreeMap::new();
        // Chains whose product is known only from a redact notice (5.3).
        let mut stated: BTreeSet<RecordId> = BTreeSet::new();
        for r in &recs {
            let k: &'static str = match r.kind() {
                names::PRODUCT => names::PRODUCT,
                names::LISTING => names::LISTING,
                names::REVIEW => names::REVIEW,
                names::REDACT => {
                    if let Some(Body::Redact(b)) = &r.body()
                        && b.product.as_ref().map(|p| p.as_str()) == Some(key)
                        && self.active(r)
                        && self.validity(&r.id()).is_valid()
                        && let Some(t) = self.rec(&b.target)
                        && !t.has_body()
                        && (t.kind() == names::LISTING || t.kind() == names::REVIEW)
                    {
                        let root = self.root_of(&b.target);
                        if self.chain_base(&root).is_some_and(|c| c.reference.is_none()) {
                            stated.insert(root);
                            let k = if t.kind() == names::LISTING { names::LISTING } else { names::REVIEW };
                            roots.insert((k, root), ());
                        }
                    }
                    continue;
                }
                _ => continue,
            };
            roots.insert((k, self.root_of(&r.id())), ());
        }
        let mut products = Vec::new();
        let mut listings = Vec::new();
        let mut reviews = Vec::new();
        for (k, root) in roots.keys() {
            let Some(c) = self.chain(root) else { continue };
            match *k {
                names::PRODUCT => {
                    if self.chain_product_key(&c.base).as_deref() == Some(key) {
                        products.push(c);
                    }
                }
                _ => {
                    if reference_product(&c.base).map(|p| p.as_str()) == Some(key) || stated.contains(root) {
                        if *k == names::LISTING { listings.push(c) } else { reviews.push(c) }
                    }
                }
            }
        }
        let p = Arc::new(ProductRecords { products, listings, reviews });
        self.c.by_product_cache.borrow_mut().insert(key.to_owned(), p.clone());
        p
    }

    /// Product key of a product chain, from its reference version (10.2).
    pub(crate) fn chain_product_key(&self, base: &ChainBase) -> Option<String> {
        let reference = base.reference.as_ref()?;
        match &reference.body() {
            Some(Body::Product(p)) => Some(p.key(base.root).as_str().to_owned()),
            _ => None,
        }
    }

    /// Persons of the sellers of a product: authors of listing chains for
    /// `key` whose reference version is active.
    pub(crate) fn seller_persons(&self, key: &str) -> Arc<BTreeSet<PersonId>> {
        if let Some(s) = self.c.seller_persons.borrow().get(key) {
            return s.clone();
        }
        let pr = self.product_records(key);
        let mut set = BTreeSet::new();
        for c in &pr.listings {
            if c.reference().is_some_and(|r| self.active(r)) {
                set.insert(self.person_id(&c.author()));
            }
        }
        let s = Arc::new(set);
        self.c.seller_persons.borrow_mut().insert(key.to_owned(), s.clone());
        s
    }

    // --- Flags and moderation -------------------------------------------------------

    /// Flags on the chain with root `root` (section 7).
    pub(crate) fn flag_info(&self, root: &RecordId) -> Arc<FlagInfo> {
        if let Some(f) = self.c.flag_infos.borrow().get(root) {
            return f.clone();
        }
        let info = Arc::new(self.compute_flags(root));
        self.c.flag_infos.borrow_mut().insert(*root, info.clone());
        info
    }

    fn compute_flags(&self, root: &RecordId) -> FlagInfo {
        let empty = FlagInfo { entries: Vec::new(), flags: Flags::default(), counted_persons: BTreeSet::new() };
        let Some(target) = self.chain_base(root) else { return empty };
        let mut flag_roots = BTreeSet::new();
        for m in &target.members {
            for f in self.s_by_target(&m.id()) {
                if f.kind() == names::FLAG {
                    flag_roots.insert(self.root_of(&f.id()));
                }
            }
        }
        if flag_roots.is_empty() {
            return empty;
        }
        let mut entries: Vec<FlagEntry> = Vec::new();
        for fr in flag_roots {
            let Some(c) = self.chain(&fr) else { continue };
            let Some(latest) = c.latest.clone() else { continue };
            let targets_root = match c.reference().and_then(|r| r.body()) {
                Some(Body::Flag(b)) => self.root_of(&b.target) == *root,
                _ => false,
            };
            if !targets_root {
                continue;
            }
            let state = if c.retract.is_some() {
                FlagState::Retracted
            } else if !latest.has_body() {
                FlagState::Unavailable
            } else {
                FlagState::Live
            };
            let excluded = match state {
                FlagState::Retracted => Some("retracted"),
                FlagState::Unavailable => Some("unavailable"),
                FlagState::Live => None,
            };
            entries.push(FlagEntry { chain: c, state, excluded });
        }
        // One flag chain per author: the one whose latest version has the highest seq.
        let mut best: BTreeMap<AuthorId, (u64, usize)> = BTreeMap::new();
        for (i, e) in entries.iter().enumerate() {
            if e.state == FlagState::Live {
                let s = e.chain.latest.as_ref().map_or(0, |l| l.seq());
                let entry = best.entry(e.chain.author()).or_insert((s, i));
                if s > entry.0 {
                    *entry = (s, i);
                }
            }
        }
        let used: BTreeSet<usize> = best.values().map(|&(_, i)| i).collect();
        for (i, e) in entries.iter_mut().enumerate() {
            if e.state == FlagState::Live && !used.contains(&i) {
                e.excluded = Some("superseded");
            }
        }
        // Counting rules 1-4.
        let target_person = self.person_id(&target.author);
        let sellers = if target.kind == names::REVIEW {
            reference_product(&target).map(|k| self.seller_persons(k.as_str()))
        } else {
            None
        };
        let mut passed: Vec<(PersonId, AuthorId, usize)> = Vec::new();
        for &i in &used {
            let a = entries[i].chain.author();
            let reason = if self.excluded_forked(&a) {
                Some("forked")
            } else {
                let p = self.person_id(&a);
                if p == target_person {
                    Some("self")
                } else if sellers.as_ref().is_some_and(|s| s.contains(&p)) {
                    Some("product_seller")
                } else if self.policy.flags_require_verified && !self.verified(&a) {
                    Some("unverified")
                } else {
                    passed.push((p, a, i));
                    None
                }
            };
            entries[i].excluded = reason;
        }
        // Rule 5: one flag per person (lowest author counts).
        passed.sort();
        let mut counted_persons = BTreeSet::new();
        for (p, _, i) in passed {
            if !counted_persons.insert(p) {
                entries[i].excluded = Some("duplicate_person");
            }
        }
        let mut flags = Flags::default();
        for &i in &used {
            flags.count += 1;
            if let Some(Body::Flag(b)) = entries[i].chain.latest_body() {
                *flags.reasons.entry(b.reason.as_str().to_owned()).or_default() += 1;
            }
        }
        flags.counted = counted_persons.len() as u64;
        entries.sort_by_key(|e| {
            let l = e.chain.latest.as_ref().expect("latest");
            (l.ts(), l.id())
        });
        FlagInfo { entries, flags, counted_persons }
    }

    /// Display status of a chain that has a latest version (section 3.6).
    pub(crate) fn display_status(&self, c: &Chain) -> Status {
        if c.retract.is_some() {
            Status::Retracted
        } else if !c.latest.as_ref().is_some_and(|l| l.has_body()) {
            Status::Unavailable
        } else if HIDEABLE.contains(&c.kind()) && self.flag_info(&c.root()).flags.counted >= self.policy.threshold(c.kind()) {
            Status::Hidden
        } else {
            Status::Visible
        }
    }

    /// The appeal of a chain (section 7.4).
    pub(crate) fn appeal(&self, c: &Chain) -> Option<Appeal> {
        let mut roots = BTreeSet::new();
        for m in &c.base.members {
            for a in self.s_by_target(&m.id()) {
                if a.kind() == names::APPEAL && a.author() == c.author() {
                    roots.insert(self.root_of(&a.id()));
                }
            }
        }
        let mut best: Option<(u64, Appeal)> = None;
        for r in roots {
            let Some(ac) = self.chain(&r) else { continue };
            if ac.retract.is_some() {
                continue;
            }
            let ok = matches!(ac.reference().and_then(|x| x.body()), Some(Body::Appeal(b)) if self.root_of(&b.target) == c.root());
            let (Some(l), true) = (ac.latest.as_ref(), ok) else { continue };
            if let Some(Body::Appeal(b)) = &l.body()
                && best.as_ref().is_none_or(|(s, _)| l.seq() > *s)
            {
                best = Some((l.seq(), Appeal { record: l.id(), note: b.note.clone(), ts: l.ts() }));
            }
        }
        best.map(|(_, a)| a)
    }

    /// The operator-stated product of a chain without any body, from the
    /// redact notice shown for it (5.3).
    pub(crate) fn stated_product(&self, c: &Chain) -> Option<ProductKey> {
        self.redaction(c)?.product
    }

    /// The redaction notice of a chain whose latest version has no body (7.5).
    pub(crate) fn redaction(&self, c: &Chain) -> Option<Redaction> {
        let latest = c.latest.as_ref()?;
        if latest.has_body() {
            return None;
        }
        let mut best: Option<((bool, u64, RecordId), Redaction)> = None;
        for n in self.s_by_target(&latest.id()) {
            if n.kind() != names::REDACT || !self.active(&n) || !self.validity(&n.id()).is_valid() {
                continue;
            }
            let Some(Body::Redact(b)) = &n.body() else { continue };
            if b.target != latest.id() {
                continue;
            }
            let k = (!self.verified(&n.author()), n.ts(), n.id());
            if best.as_ref().is_none_or(|(bk, _)| k < *bk) {
                best = Some((
                    k,
                    Redaction {
                        record: Some(n.id()),
                        by: Some(n.author()),
                        reason: Some(b.reason.as_str().to_owned()),
                        note: b.note.clone(),
                        product: b.product.clone(),
                    },
                ));
            }
        }
        Some(best.map(|(_, r)| r).unwrap_or(Redaction { record: None, by: None, reason: None, note: None, product: None }))
    }

    pub(crate) fn moderation(&self, c: &Chain) -> Moderation {
        Moderation {
            status: self.display_status(c),
            flags: self.flag_info(&c.root()).flags.clone(),
            appeal: self.appeal(c),
            redaction: self.redaction(c),
        }
    }

    // --- Restriction -------------------------------------------------------------

    /// Restriction of the person of `a` (section 6).
    pub(crate) fn restriction(&self, a: &AuthorId) -> Arc<Restriction> {
        let person = self.person(a);
        if let Some(r) = self.c.restrictions.borrow().get(&person.id) {
            return r.clone();
        }
        let mut hidden = 0u64;
        let mut flaggers = BTreeSet::new();
        let mut forked = false;
        let thr = self.policy.threshold(names::LISTING);
        for k in &person.keys {
            forked |= self.excluded_forked(k);
            // Only flagged listings can be hidden. Unflagged ones are skipped
            // without reading them, which keeps the related set small (14).
            let mut roots = BTreeSet::new();
            for id in self.store.by_author_kind(k, names::LISTING) {
                let flagged = self
                    .store
                    .by_target(&id)
                    .iter()
                    .any(|f| self.store.get(f).is_some_and(|r| r.kind() == names::FLAG));
                if flagged {
                    roots.insert(self.root_of(&id));
                }
            }
            for root in roots {
                let Some(base) = self.chain_base(&root) else { continue };
                if !base.reference.as_ref().is_some_and(|r| self.active(r)) {
                    continue;
                }
                let fi = self.flag_info(&root);
                if fi.flags.counted >= thr {
                    hidden += 1;
                    flaggers.extend(fi.counted_persons.iter().cloned());
                }
            }
        }
        let distinct = flaggers.len() as u64;
        let restricted = forked
            || (hidden >= self.policy.seller_hidden_listing_limit && distinct >= self.policy.seller_min_distinct_flaggers);
        let r = Arc::new(Restriction { hidden_listings: hidden, distinct_flaggers: distinct, restricted });
        self.c.restrictions.borrow_mut().insert(person.id.clone(), r.clone());
        r
    }

    pub(crate) fn restricted(&self, a: &AuthorId) -> bool {
        self.restriction(a).restricted
    }

    // --- Profiles ---------------------------------------------------------------

    /// The author's profile chain (section 10.1).
    pub(crate) fn profile(&self, a: &AuthorId) -> Option<Arc<Chain>> {
        if let Some(p) = self.c.profiles.borrow().get(a) {
            return p.clone();
        }
        let mut roots = BTreeSet::new();
        for r in self.s_by_author_kind(a, names::PROFILE) {
            roots.insert(self.root_of(&r.id()));
        }
        let mut best: Option<Arc<Chain>> = None;
        for root in roots {
            let Some(c) = self.chain(&root) else { continue };
            let Some(l) = &c.latest else { continue };
            if best.as_ref().is_none_or(|b| l.seq() > b.latest.as_ref().map_or(0, |x| x.seq())) {
                best = Some(c);
            }
        }
        self.c.profiles.borrow_mut().insert(*a, best.clone());
        best
    }

    /// Profile name for display inside other views (visible profiles only).
    pub(crate) fn display_name(&self, a: &AuthorId) -> Option<String> {
        let c = self.profile(a)?;
        if self.display_status(&c) != Status::Visible {
            return None;
        }
        match c.latest_body() {
            Some(Body::Profile(p)) => Some(p.name.clone()),
            _ => None,
        }
    }

    // --- Listings helpers ----------------------------------------------------------

    /// ASCII host of a record's `url` body field.
    pub(crate) fn host(&self, r: &Rec) -> Option<String> {
        if let Some(h) = self.c.hosts.borrow().get(&r.id()) {
            return h.clone();
        }
        let url = match &r.body() {
            Some(Body::Listing(l)) => Some(l.url.as_str()),
            _ => None,
        };
        let h = url.and_then(|u| perch_core::https_url_host(u).ok());
        self.c.hosts.borrow_mut().insert(r.id(), h.clone());
        h
    }

    /// The matching domain attestation of a listing's latest version (5.5).
    pub(crate) fn listing_domain(&self, c: &Chain) -> Option<DomAtt> {
        let l = c.latest.as_ref()?;
        let host = self.host(l)?;
        let ident = self.ident(&c.author());
        ident
            .domains
            .iter()
            .filter(|d| perch_core::weburl::host_matches_domain(&host, &d.domain))
            .min_by(|x, y| {
                (std::cmp::Reverse(x.domain.len()), x.attestor, x.root).cmp(&(std::cmp::Reverse(y.domain.len()), y.attestor, y.root))
            })
            .cloned()
    }

    // --- Reviews -----------------------------------------------------------------

    /// Review chains of a product and their counting outcome (9.3).
    pub(crate) fn review_set(&self, key: &str) -> Arc<ReviewSet> {
        if let Some(s) = self.c.review_sets.borrow().get(key) {
            return s.clone();
        }
        let pr = self.product_records(key);
        let sellers = self.seller_persons(key);
        let mut evals: Vec<ReviewEval> = Vec::new();
        let mut revoked = 0;
        for c in &pr.reviews {
            if c.latest.is_none() {
                let cut = self.cuts(&c.author()).revoke_cut;
                if cut != NONE && c.base.members.iter().filter(|m| m.has_body()).all(|m| m.seq() > cut) {
                    revoked += 1;
                }
                continue;
            }
            let latest = c.latest.as_ref().expect("latest");
            let excluded = if c.retract.is_some() {
                Some("retracted")
            } else if !latest.has_body() {
                Some("unavailable")
            } else if self.excluded_forked(&c.author()) {
                Some("forked")
            } else if sellers.contains(&self.person_id(&c.author())) {
                Some("seller")
            } else {
                None
            };
            evals.push(ReviewEval { chain: c.clone(), excluded });
        }
        // Rule 5: latest chain per author.
        let mut by_author: BTreeMap<AuthorId, (u64, usize)> = BTreeMap::new();
        for (i, e) in evals.iter().enumerate() {
            if e.excluded.is_none() {
                let s = e.chain.latest.as_ref().map_or(0, |l| l.seq());
                let x = by_author.entry(e.chain.author()).or_insert((s, i));
                if s > x.0 {
                    *x = (s, i);
                }
            }
        }
        let keep: BTreeSet<usize> = by_author.values().map(|&(_, i)| i).collect();
        for (i, e) in evals.iter_mut().enumerate() {
            if e.excluded.is_none() && !keep.contains(&i) {
                e.excluded = Some("superseded");
            }
        }
        // Rule 6: one chain per person: highest latest ts, then lowest author, then lowest root.
        let mut by_person: BTreeMap<PersonId, usize> = BTreeMap::new();
        for &i in &keep {
            let p = self.person_id(&evals[i].chain.author());
            let k = |j: usize| {
                let c = &evals[j].chain;
                (std::cmp::Reverse(c.latest.as_ref().map_or(0, |l| l.ts())), c.author(), c.root())
            };
            match by_person.get(&p) {
                Some(&j) if k(j) <= k(i) => {}
                _ => {
                    by_person.insert(p, i);
                }
            }
        }
        let winners: BTreeSet<usize> = by_person.values().copied().collect();
        for &i in &keep {
            if !winners.contains(&i) {
                evals[i].excluded = Some("superseded");
            }
        }
        let s = Arc::new(ReviewSet { evals, revoked });
        self.c.review_sets.borrow_mut().insert(key.to_owned(), s.clone());
        s
    }

    /// Verified-purchase outcome of a review chain (9.2).
    pub(crate) fn purchase(&self, c: &Chain) -> Purchase {
        if let Some(p) = self.c.purchases.borrow().get(&c.root()) {
            return *p;
        }
        let p = self.compute_purchase(c);
        self.c.purchases.borrow_mut().insert(c.root(), p);
        p
    }

    fn compute_purchase(&self, c: &Chain) -> Purchase {
        let invalid = Purchase { outcome: "receipt_invalid", receipt_seller: None };
        let Some(Body::Review(review)) = c.latest_body() else {
            return Purchase { outcome: "none", receipt_seller: None };
        };
        let Some(rc) = &review.receipt else {
            return Purchase { outcome: "none", receipt_seller: None };
        };
        let author = c.author();
        let Some(l) = self.rec(&rc.listing) else { return invalid };
        if l.kind() != names::LISTING {
            return invalid;
        }
        let lroot = self.root_of(&rc.listing);
        let Some(lbase) = self.chain_base(&lroot) else { return invalid };
        let root_rec = &lbase.members[0];
        if !self.active(root_rec) {
            return invalid;
        }
        let Some(lprod) = reference_product(&lbase).cloned() else { return invalid };
        let seller = lbase.author;
        if rc.verify(&seller).is_err() || rc.buyer != author {
            return invalid;
        }
        let ok = |o| Purchase { outcome: o, receipt_seller: Some(seller) };
        if lprod != review.product {
            return ok("wrong_product");
        }
        if self.person_id(&author) == self.person_id(&seller) {
            return ok("self");
        }
        if !self.verified(&seller) {
            return ok("seller_unverified");
        }
        if self.policy.verified_purchase_requires_verified_buyer && !self.verified(&author) {
            return ok("buyer_unverified");
        }
        ok("verified")
    }
}
