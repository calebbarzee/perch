//! Admission tiers (ARCHITECTURE 9, docs/sync-protocol.md 9.2).

use std::collections::HashMap;
use std::sync::Mutex;

use perch_core::AuthorId;
use perch_core::kinds::names;
use perch_store::{AdmissionLimit, Change, MemoryStore, Store};
use perch_views::{Policy, Tier};

use crate::node::now_ms;

/// Per-author limits of each tier and the tier-3 node budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TierLimits {
    /// Tier 1, summed over all keys of one person.
    pub t1_records: u64,
    pub t1_bytes: u64,
    pub t2_records: u64,
    pub t2_bytes: u64,
    pub t3_records: u64,
    pub t3_bytes: u64,
    /// Total bytes of all tier-3 logs.
    pub t3_budget: u64,
}

impl Default for TierLimits {
    fn default() -> Self {
        TierLimits {
            t1_records: 1_000_000,
            t1_bytes: 1 << 30,
            t2_records: 1_000,
            t2_bytes: 16 << 20,
            t3_records: 100,
            t3_bytes: 1 << 20,
            t3_budget: 1 << 30,
        }
    }
}

/// Tier-3 admission rank: `sha256(author ‖ floor(day / 30))`, where `author`
/// is the 32 key bytes, `day = floor(now_ms / 86,400,000)`, and the epoch is
/// encoded as 8 bytes big-endian. Lower ranks are admitted first.
pub fn tier3_rank(author: &AuthorId, now_ms: u64) -> [u8; 32] {
    let epoch = now_ms / 86_400_000 / 30;
    let mut buf = Vec::with_capacity(40);
    buf.extend_from_slice(author.as_bytes());
    buf.extend_from_slice(&epoch.to_be_bytes());
    perch_core::sha256(&buf)
}

/// Computes tiers and limits. Tier-1 results are cached; the cache is
/// cleared when records that can change a tier arrive (attestations, flags,
/// appeals, reviews, retractions, forks) and at every maintenance tick.
/// Tiers 2 and 3 are computed on every call, so a rise is seen by the next
/// record even within one batch; their cost is bounded by their log limits.
pub struct Admission {
    policy: Policy,
    pub limits: TierLimits,
    cache: Mutex<HashMap<AuthorId, Tier>>,
}

impl Admission {
    pub fn new(policy: Policy, limits: TierLimits) -> Admission {
        Admission { policy, limits, cache: Mutex::new(HashMap::new()) }
    }

    fn policy_now(&self) -> Policy {
        let mut p = self.policy.clone();
        p.now = now_ms();
        p
    }

    pub fn tier(&self, store: &MemoryStore, author: &AuthorId) -> Tier {
        if let Some(t) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).get(author) {
            return *t;
        }
        let t = perch_views::admission_tier(store, &self.policy_now(), author);
        if t == Tier::One {
            self.cache.lock().unwrap_or_else(|e| e.into_inner()).insert(*author, t);
        }
        t
    }

    pub fn limit(&self, store: &MemoryStore, author: &AuthorId) -> AdmissionLimit {
        let l = &self.limits;
        match self.tier(store, author) {
            Tier::One => {
                // Per person: the other keys of the person use part of the limit.
                let keys = perch_views::person_keys(store, &self.policy_now(), author);
                let (mut recs, mut bytes) = (0u64, 0u64);
                for k in keys.iter().filter(|k| *k != author) {
                    recs += store.head(k).map_or(0, |h| h.seq);
                    bytes += store.log_bytes(k);
                }
                AdmissionLimit {
                    records: l.t1_records.saturating_sub(recs),
                    bytes: l.t1_bytes.saturating_sub(bytes),
                    pool_rank: None,
                }
            }
            Tier::Two => AdmissionLimit { records: l.t2_records, bytes: l.t2_bytes, pool_rank: None },
            Tier::Three => AdmissionLimit {
                records: l.t3_records,
                bytes: l.t3_bytes,
                pool_rank: Some(tier3_rank(author, now_ms())),
            },
        }
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
