//! Adapter between the adversarial tests and the crates under test.
//!
//! Every test calls only these functions. When a crate API changes, only this
//! file changes. Inputs and outputs are wire strings and JSON values, as in the
//! contract, so the tests stay independent of internal Rust types.

use serde_json::Value;

/// Outcome of offering one record to a store (contract 4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ingest {
    Inserted,
    Duplicate,
    Gap,
    Fork,
    /// Isolated-invalid or log-level refusal (for example a `ts` decrease).
    Rejected(String),
}

/// Isolated verification (perch-core): returns the record ID or an error.
pub fn verify(line: &str) -> Result<String, String> {
    imp::verify(line)
}

/// Normalize a valid input to its canonical wire form (contract 4.2).
pub fn normalize(line: &str) -> Result<String, String> {
    imp::normalize(line)
}

/// Strict detached-receipt verification under `seller` (contract 4.7).
pub fn verify_receipt(receipt: &Value, seller: &str) -> Result<(), String> {
    imp::verify_receipt(receipt, seller)
}

/// A store under test (perch-store).
pub struct Store(imp::Store);

impl Store {
    pub fn new() -> Store {
        Store(imp::Store::new())
    }
    pub fn ingest(&mut self, line: &str) -> Ingest {
        self.0.ingest(line)
    }
    pub fn ingest_all<S: AsRef<str>>(&mut self, lines: &[S]) -> Vec<Ingest> {
        lines.iter().map(|l| self.ingest(l.as_ref())).collect()
    }
    /// Highest contiguous seq stored for an author (0 if none).
    pub fn head_seq(&self, author: &str) -> i64 {
        self.0.head_seq(author)
    }
    /// Fork seq for an author, if forked.
    pub fn fork_seq(&self, author: &str) -> Option<i64> {
        self.0.fork_seq(author)
    }
    /// IDs of the stored evidence pair for an author, sorted as in contract 5.2.
    pub fn evidence(&self, author: &str) -> Option<(String, String)> {
        self.0.evidence(author)
    }
    pub fn has(&self, id: &str) -> bool {
        self.0.has(id)
    }
    /// A sparse store (contract 5.5).
    pub fn new_sparse() -> Store {
        Store(imp::Store::new_sparse())
    }
    /// True if the record is held with its body.
    pub fn has_body(&self, id: &str) -> bool {
        self.0.has_body(id)
    }
    /// Remove a record's body (contract 5.3). Err for control kinds.
    pub fn remove_body(&mut self, id: &str) -> Result<bool, String> {
        self.0.remove_body(id)
    }
    /// Put an ID on the node's removal list.
    pub fn add_removed(&mut self, id: &str) {
        self.0.add_removed(id)
    }
    /// Canonical JSONL export.
    pub fn export(&self) -> String {
        self.0.export()
    }
}

impl Default for Store {
    fn default() -> Self {
        Store::new()
    }
}

/// Views (perch-views). Each returns the JSON shape of contract 7.8, or null.
pub fn product_view(store: &Store, policy: &Value, key: &str) -> Value {
    imp::product_view(&store.0, policy, key)
}
pub fn listing_view(store: &Store, policy: &Value, id: &str) -> Value {
    imp::listing_view(&store.0, policy, id)
}
pub fn author_view(store: &Store, policy: &Value, id: &str) -> Value {
    imp::author_view(&store.0, policy, id)
}
pub fn search(store: &Store, policy: &Value, q: &str) -> Value {
    imp::search(&store.0, policy, q)
}

/// Two sync peers exchanging messages until quiet (perch-sync). The function
/// passes messages through `filter`, which can drop, reorder, or rewrite them,
/// to model a hostile peer. Returns when no messages remain or after `max_rounds`.
pub fn sync_pair(a: &mut Store, b: &mut Store, max_rounds: usize, filter: &mut dyn FnMut(Dir, String) -> Vec<String>) {
    imp::sync_pair(&mut a.0, &mut b.0, max_rounds, filter)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    AtoB,
    BtoA,
}

/// Layers of the system under test, wired one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Core,
    Store,
    Views,
    Sync,
}

/// Whether a layer is wired into this adapter. Tests that need an unwired
/// layer are skipped with a message.
pub fn available(layer: Layer) -> bool {
    match layer {
        Layer::Core => imp::CORE,
        Layer::Store => imp::STORE,
        Layer::Views => imp::VIEWS,
        Layer::Sync => imp::SYNC,
    }
}

/// Skip the test unless the given layer (default: views) is wired.
#[macro_export]
macro_rules! require_sut {
    () => {
        $crate::require_sut!(Views)
    };
    ($layer:ident) => {
        if !$crate::sut::available($crate::sut::Layer::$layer) {
            eprintln!("SKIP: layer {} not wired into perch-tests/src/sut.rs yet", stringify!($layer));
            return;
        }
    };
}

// ---------------------------------------------------------------------------
// Implementation. Replace this module with calls into the real crates.
// ---------------------------------------------------------------------------
mod imp {
    use super::{Dir, Ingest};
    use perch_core::{AuthorId, Record};
    use serde_json::Value;

    pub const CORE: bool = true;
    pub const STORE: bool = true;
    pub const VIEWS: bool = false;
    pub const SYNC: bool = false;

    pub fn verify(line: &str) -> Result<String, String> {
        Record::from_json_str(line).map(|r| r.id().to_string()).map_err(|e| e.to_string())
    }
    pub fn normalize(line: &str) -> Result<String, String> {
        Record::from_json_str(line).map(|r| r.to_json_string()).map_err(|e| e.to_string())
    }
    pub fn verify_receipt(r: &Value, seller: &str) -> Result<(), String> {
        let rc = perch_core::receipt::parse_receipt(r).map_err(|e| e.to_string())?;
        let seller = AuthorId::from_hex(seller).map_err(|e| e.to_string())?;
        perch_core::verify_receipt(&rc, &seller).map_err(|e| e.to_string())
    }

    pub struct Store(pub perch_store::MemoryStore);
    impl Store {
        pub fn new() -> Store {
            Store(perch_store::MemoryStore::new())
        }
        pub fn new_sparse() -> Store {
            Store(perch_store::MemoryStore::new_sparse())
        }
        pub fn ingest(&mut self, line: &str) -> Ingest {
            use perch_store::Insert;
            match self.0.insert_json(line) {
                Err(e) => Ingest::Rejected(e.to_string()),
                Ok(Insert::Inserted) | Ok(Insert::BodyFilled) => Ingest::Inserted,
                Ok(Insert::Duplicate) => Ingest::Duplicate,
                Ok(Insert::Gap) => Ingest::Gap,
                Ok(Insert::Fork { .. }) => Ingest::Fork,
                Ok(Insert::AboveFork) => Ingest::Rejected("above_fork".into()),
                Ok(Insert::Rejected(r)) => Ingest::Rejected(r.to_string()),
            }
        }
        fn author(a: &str) -> Option<perch_core::AuthorId> {
            perch_core::AuthorId::from_hex(a).ok()
        }
        pub fn head_seq(&self, a: &str) -> i64 {
            use perch_store::Store as _;
            Self::author(a).and_then(|a| self.0.head(&a)).map_or(0, |h| h.seq as i64)
        }
        pub fn fork_seq(&self, a: &str) -> Option<i64> {
            use perch_store::Store as _;
            Self::author(a).and_then(|a| self.0.fork_seq(&a)).map(|s| s as i64)
        }
        pub fn evidence(&self, a: &str) -> Option<(String, String)> {
            use perch_store::Store as _;
            let p = self.0.evidence(&Self::author(a)?)?;
            let [x, y] = p.ids();
            Some((x.to_hex(), y.to_hex()))
        }
        pub fn has(&self, id: &str) -> bool {
            use perch_store::Store as _;
            perch_core::RecordId::from_hex(id).is_ok_and(|id| self.0.contains(&id))
        }
        pub fn has_body(&self, id: &str) -> bool {
            use perch_store::Store as _;
            perch_core::RecordId::from_hex(id).is_ok_and(|id| self.0.body(&id).is_some())
        }
        pub fn remove_body(&mut self, id: &str) -> Result<bool, String> {
            use perch_store::Store as _;
            let id = perch_core::RecordId::from_hex(id).map_err(|e| e.to_string())?;
            self.0.remove_body(&id).map_err(|e| e.to_string())
        }
        pub fn add_removed(&mut self, id: &str) {
            if let Ok(id) = perch_core::RecordId::from_hex(id) {
                self.0.add_removed([id]);
            }
        }
        pub fn export(&self) -> String {
            self.0.export_jsonl()
        }
    }

    pub fn product_view(_s: &Store, _p: &Value, _k: &str) -> Value {
        Value::Null
    }
    pub fn listing_view(_s: &Store, _p: &Value, _id: &str) -> Value {
        Value::Null
    }
    pub fn author_view(_s: &Store, _p: &Value, _id: &str) -> Value {
        Value::Null
    }
    pub fn search(_s: &Store, _p: &Value, _q: &str) -> Value {
        Value::Null
    }
    pub fn sync_pair(_a: &mut Store, _b: &mut Store, _m: usize, _f: &mut dyn FnMut(Dir, String) -> Vec<String>) {}
}
