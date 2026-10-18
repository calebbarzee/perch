//! Secondary indexes. Each stored log record contributes a set of
//! [`IndexKeys`], computed from the record's current state (header, body if
//! held, and its chain root). The store keeps the keys per record so it can
//! remove them exactly.

use std::collections::{BTreeMap, BTreeSet};

use perch_core::kinds::names;
use perch_core::{AuthorId, Record, RecordId, kinds};
use serde_json::{Map, Value};

/// Index entries of one record.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct IndexKeys {
    pub product: Option<String>,
    pub target: Option<RecordId>,
    pub subject: Option<AuthorId>,
    /// IDs this record references by `body.listing`, `body.receipt.listing`, or `replaces`.
    pub refs: Vec<RecordId>,
}

fn hex_id(v: Option<&Value>) -> Option<RecordId> {
    match v {
        Some(Value::String(s)) => RecordId::from_hex(s).ok(),
        _ => None,
    }
}

fn hex_author(v: Option<&Value>) -> Option<AuthorId> {
    match v {
        Some(Value::String(s)) => AuthorId::from_hex(s).ok(),
        _ => None,
    }
}

/// Product key of a `product` record, given its chain root.
pub(crate) fn product_key(body: Option<&Map<String, Value>>, root: RecordId) -> String {
    match body.map(|b| kinds::derive_product_key(b, root)) {
        Some(Ok(k)) => k.as_str().to_string(),
        _ => perch_core::ProductKey::from_root(root).as_str().to_string(),
    }
}

/// Computes the index keys of a record. `root` is the root ID of its chain.
pub(crate) fn keys_for(r: &Record, root: RecordId) -> IndexKeys {
    let mut k = IndexKeys::default();
    if let Some(rep) = r.replaces() {
        k.refs.push(rep);
    }
    let kind = r.kind();
    if kind == names::PRODUCT {
        k.product = Some(product_key(r.body(), root));
    }
    let Some(body) = r.body() else {
        return k;
    };
    match kind {
        names::LISTING | names::OBSERVATION | names::REVIEW => {
            if let Some(Value::String(p)) = body.get("product")
                && !p.is_empty()
                && p.len() <= 1024
            {
                k.product = Some(p.clone());
            }
        }
        names::FLAG | names::APPEAL | names::RETRACT | names::REDACT => {
            k.target = hex_id(body.get("target"));
        }
        names::ATTEST_IDENTITY | names::ATTEST_DOMAIN | names::ATTEST_REVOKE => {
            k.subject = hex_author(body.get("subject"));
        }
        _ => {}
    }
    if matches!(kind, names::OBSERVATION | names::REVIEW)
        && let Some(l) = hex_id(body.get("listing"))
    {
        k.refs.push(l);
    }
    if kind == names::REVIEW
        && let Some(Value::Object(rc)) = body.get("receipt")
        && let Some(l) = hex_id(rc.get("listing"))
    {
        k.refs.push(l);
    }
    k.refs.sort();
    k.refs.dedup();
    k
}

/// All secondary indexes.
#[derive(Clone, Debug, Default)]
pub(crate) struct Indexes {
    pub by_kind: BTreeMap<String, BTreeSet<RecordId>>,
    pub by_author_kind: BTreeMap<(AuthorId, String), BTreeMap<u64, RecordId>>,
    pub by_product: BTreeMap<String, BTreeSet<RecordId>>,
    pub by_target: BTreeMap<RecordId, BTreeSet<RecordId>>,
    pub by_subject: BTreeMap<AuthorId, BTreeSet<RecordId>>,
    pub referencing: BTreeMap<RecordId, BTreeSet<RecordId>>,
    pub replaced_by: BTreeMap<RecordId, BTreeSet<RecordId>>,
}

fn add<K: Ord + Clone>(m: &mut BTreeMap<K, BTreeSet<RecordId>>, k: &K, id: RecordId) {
    m.entry(k.clone()).or_default().insert(id);
}

fn del<K: Ord>(m: &mut BTreeMap<K, BTreeSet<RecordId>>, k: &K, id: &RecordId) {
    if let Some(s) = m.get_mut(k) {
        s.remove(id);
        if s.is_empty() {
            m.remove(k);
        }
    }
}

impl Indexes {
    pub fn add(&mut self, r: &Record, keys: &IndexKeys) {
        let id = r.id();
        add(&mut self.by_kind, &r.kind().to_string(), id);
        self.by_author_kind.entry((r.author(), r.kind().to_string())).or_default().insert(r.seq(), id);
        if let Some(p) = &keys.product {
            add(&mut self.by_product, p, id);
        }
        if let Some(t) = &keys.target {
            add(&mut self.by_target, t, id);
        }
        if let Some(s) = &keys.subject {
            add(&mut self.by_subject, s, id);
        }
        for x in &keys.refs {
            add(&mut self.referencing, x, id);
        }
        if let Some(rep) = r.replaces() {
            add(&mut self.replaced_by, &rep, id);
        }
    }

    pub fn remove(&mut self, r: &Record, keys: &IndexKeys) {
        let id = r.id();
        del(&mut self.by_kind, &r.kind().to_string(), &id);
        let ak = (r.author(), r.kind().to_string());
        if let Some(m) = self.by_author_kind.get_mut(&ak) {
            if m.get(&r.seq()) == Some(&id) {
                m.remove(&r.seq());
            }
            if m.is_empty() {
                self.by_author_kind.remove(&ak);
            }
        }
        if let Some(p) = &keys.product {
            del(&mut self.by_product, p, &id);
        }
        if let Some(t) = &keys.target {
            del(&mut self.by_target, t, &id);
        }
        if let Some(s) = &keys.subject {
            del(&mut self.by_subject, s, &id);
        }
        for x in &keys.refs {
            del(&mut self.referencing, x, &id);
        }
        if let Some(rep) = r.replaces() {
            del(&mut self.replaced_by, &rep, &id);
        }
    }
}
