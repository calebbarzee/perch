//! # perch-views
//!
//! Deterministic views over Perch records and the [`Policy`] type
//! ([ARCHITECTURE](../../../ARCHITECTURE.md) section 7; specification in
//! `docs/views.md`).
//!
//! A view is a pure function of (store contents, policy). Views read no
//! clock (`policy.now` supplies the time), use no floating-point
//! arithmetic, and sort every list totally, so native and wasm builds give
//! byte-identical [`canonical_json`] output.
//!
//! The free functions create a fresh [`Views`] context per call. To compute
//! several views over the same store and policy, create one [`Views`] and
//! call its methods; it memoizes intermediate results.

#![warn(missing_docs)]

mod canon;
mod engine;
mod misc;
pub mod model;
pub mod num;
pub mod policy;
mod search;
mod views;

use perch_core::{AuthorId, RecordId};
use perch_store::Store;

pub use canon::canonical_json;
pub use engine::{ViewCache, Views};
pub use misc::Tier;
pub use model::*;
pub use policy::{ATTESTOR_KINDS, Policy, PolicyError, TrustedAttestor, default_policy, policy_hash};
pub use search::{DEFAULT_LIMIT, MAX_LIMIT, SearchOptions, Sort, query_tokens, tokens};

/// `ProductView` for a product key, or `None` if nothing is known about it.
pub fn product_view<S: Store + ?Sized>(store: &S, policy: &Policy, key: &str) -> Option<ProductView> {
    Views::new(store, policy).product(key)
}

/// `ListingView` for any member ID of a listing chain.
pub fn listing_view<S: Store + ?Sized>(store: &S, policy: &Policy, id: &RecordId) -> Option<ListingView> {
    Views::new(store, policy).listing(id)
}

/// `AuthorView`.
pub fn author_view<S: Store + ?Sized>(store: &S, policy: &Policy, author: &AuthorId) -> Option<AuthorView> {
    Views::new(store, policy).author(author)
}

/// Search hits for a query.
pub fn search<S: Store + ?Sized>(store: &S, policy: &Policy, q: &str, opts: &SearchOptions) -> Vec<SearchHit> {
    Views::new(store, policy).search(q, opts)
}

/// Store counts. Does not depend on the policy.
pub fn stats<S: Store + ?Sized>(store: &S) -> Stats {
    let p = default_policy(0, &[]);
    Views::new(store, &p).stats()
}

/// Every semantically invalid record with its reason code, sorted by ID.
/// Does not depend on the policy.
pub fn invalid_records<S: Store + ?Sized>(store: &S) -> Vec<(RecordId, String)> {
    let p = default_policy(0, &[]);
    Views::new(store, &p).invalid_records()
}

/// Flags on the chain of `id`.
pub fn flags<S: Store + ?Sized>(store: &S, policy: &Policy, id: &RecordId) -> Vec<FlagView> {
    Views::new(store, policy).flags(id)
}

/// Chains of an author, by root seq.
pub fn author_records<S: Store + ?Sized>(
    store: &S,
    policy: &Policy,
    author: &AuthorId,
    kinds: &[&str],
    limit: usize,
    from_seq: u64,
) -> Vec<ChainSummary> {
    Views::new(store, policy).author_records(author, kinds, limit, from_seq)
}

/// Validity and chain of one record.
pub fn record_status<S: Store + ?Sized>(store: &S, policy: &Policy, id: &RecordId) -> Option<RecordStatus> {
    Views::new(store, policy).record_status(id)
}

/// True if `author` is verified under `policy`.
pub fn is_verified<S: Store + ?Sized>(store: &S, policy: &Policy, author: &AuthorId) -> bool {
    Views::new(store, policy).verified_author(author)
}

/// Display status of the chain of `id`, if it has a latest version.
pub fn chain_status<S: Store + ?Sized>(store: &S, policy: &Policy, id: &RecordId) -> Option<Status> {
    Views::new(store, policy).chain_status(id)
}

/// The related set of a product (ARCHITECTURE 8.1, docs/views.md section
/// 14): every record ID that [`product_view`] reads, including fork
/// evidence records. A sparse store holding these records gives the same
/// `ProductView` as the full store, under this policy.
pub fn related_records<S: Store + ?Sized>(store: &S, key: &str, policy: &Policy) -> Vec<RecordId> {
    let v = Views::traced(store, policy);
    let _ = v.product(key);
    // Names of the trusted attestors, so a light client can say who verified
    // an author (ARCHITECTURE 8.1: profiles of every involved author,
    // including attestors).
    for t in &policy.trusted_attestors {
        let _ = v.display_name(&t.author);
    }
    v.take_trace()
}

/// Keys of the person of `author` (7.3), ascending, including `author`.
/// An author with no counting identity attestation gets `[author]`.
pub fn person_keys<S: Store + ?Sized>(store: &S, policy: &Policy, author: &AuthorId) -> Vec<AuthorId> {
    Views::new(store, policy).person_keys(author)
}

/// Admission tier of `author`; `admission` is a policy built from the
/// node's admission attestor set (ARCHITECTURE 9).
pub fn admission_tier<S: Store + ?Sized>(store: &S, admission: &Policy, author: &AuthorId) -> Tier {
    Views::new(store, admission).admission_tier(author)
}

/// True if a tier-1 author's active record references record `id`.
pub fn referenced_by_tier1<S: Store + ?Sized>(store: &S, admission: &Policy, id: &RecordId) -> bool {
    Views::new(store, admission).referenced_by_tier1(id)
}

impl<S: Store + ?Sized> Views<'_, S> {
    /// True if `author` is verified.
    pub fn verified_author(&self, author: &AuthorId) -> bool {
        self.verified(author)
    }

    /// Keys of the person of `author`, ascending, including `author`.
    pub fn person_keys(&self, author: &AuthorId) -> Vec<AuthorId> {
        self.person(author).keys.clone()
    }

    /// Display status of the chain of `id`.
    pub fn chain_status(&self, id: &RecordId) -> Option<Status> {
        let c = self.chain_of(id)?;
        c.latest.as_ref()?;
        Some(self.display_status(&c))
    }
}
