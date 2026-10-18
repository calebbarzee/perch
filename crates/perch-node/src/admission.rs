//! Admission tiers (ARCHITECTURE 9, docs/sync-protocol.md 9.2).

use std::collections::HashMap;
use std::sync::Mutex;

use perch_core::AuthorId;
use perch_core::kinds::names;
use perch_store::{AdmissionLimit, Change, MemoryStore};
use perch_views::{Policy, Tier};

use crate::node::now_ms;

/// Per-author limit of each tier.
pub fn tier_limit(t: Tier) -> AdmissionLimit {
    match t {
        Tier::One => AdmissionLimit { records: 1_000_000, bytes: 1 << 30 },
        Tier::Two => AdmissionLimit { records: 1_000, bytes: 16 << 20 },
        Tier::Three => AdmissionLimit { records: 100, bytes: 1 << 20 },
    }
}

/// Computes tiers. Tier-1 results are cached; the cache is cleared when
/// records that can change a tier arrive (attestations, flags, appeals,
/// reviews, retractions, forks). Tiers 2 and 3 are computed on every call, so
/// a rise is seen by the next record even within one batch; the cost is
/// bounded by their log limits.
pub struct Admission {
    policy: Policy,
    cache: Mutex<HashMap<AuthorId, Tier>>,
}

impl Admission {
    pub fn new(policy: Policy) -> Admission {
        Admission { policy, cache: Mutex::new(HashMap::new()) }
    }

    pub fn tier(&self, store: &MemoryStore, author: &AuthorId) -> Tier {
        if let Some(t) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).get(author) {
            return *t;
        }
        let mut p = self.policy.clone();
        p.now = now_ms();
        let t = perch_views::admission_tier(store, &p, author);
        if t == Tier::One {
            self.cache.lock().unwrap_or_else(|e| e.into_inner()).insert(*author, t);
        }
        t
    }

    pub fn limit(&self, store: &MemoryStore, author: &AuthorId) -> AdmissionLimit {
        tier_limit(self.tier(store, author))
    }

    pub fn clear(&self) {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// Clears the cache if a change can raise or lower a tier.
    pub fn on_changes(&self, changes: &[Change]) {
        let relevant = changes.iter().any(|c| match c {
            Change::Inserted(r) | Change::BodyFilled(r) => matches!(
                r.kind(),
                names::ATTEST_IDENTITY
                    | names::ATTEST_DOMAIN
                    | names::ATTEST_REVOKE
                    | names::FLAG
                    | names::APPEAL
                    | names::REVIEW
                    | names::RETRACT
            ),
            Change::PairChanged(_) => true,
            Change::BodyRemoved(_) => false,
        });
        if relevant {
            self.clear();
        }
    }
}
