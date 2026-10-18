//! Author view, author records, flags, record status, stats, invalid
//! records, related set, and admission tier (docs/views.md sections 14, 15).

use std::collections::BTreeSet;

use perch_core::kinds::{Body, names};
use perch_core::{AuthorId, RecordId};
use perch_store::Store;

use crate::engine::{FlagState, HIDEABLE, Validity, Views, reference_product};
use crate::model::*;

/// Admission tier (ARCHITECTURE 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub enum Tier {
    /// Verified authors and attestors.
    One = 1,
    /// Unverified authors referenced by tier-1 records.
    Two = 2,
    /// All others.
    Three = 3,
}

impl<S: Store + ?Sized> Views<'_, S> {
    /// `AuthorView` (15.1), or `None` for an author the store knows nothing about.
    pub fn author(&self, a: &AuthorId) -> Option<AuthorView> {
        if self.store.head(a).is_none() && self.store.by_subject(a).is_empty() {
            return None;
        }
        let ident = self.ident(a);
        let cuts = self.cuts(a);
        let profile = self.profile(a);
        let (mut name, mut role, mut url, mut about) = (None, None, None, None);
        let mut profile_mod = None;
        let mut profile_status = None;
        if let Some(c) = &profile {
            let m = self.moderation(c);
            if matches!(m.status, Status::Visible | Status::Hidden)
                && let Some(Body::Profile(p)) = c.latest_body()
            {
                name = Some(p.name.clone());
                role = Some(p.role.as_str().to_owned());
                url = p.url.clone();
                about = p.about.clone();
            }
            profile_status = Some(m.status);
            profile_mod = Some(m);
        }
        let verified = ident.ids.first().map(|i| Verified {
            level: i.level.to_owned(),
            legal_name: i.legal_name.clone(),
            country: i.country.clone(),
            attestor: i.attestor,
            expires: i.expires,
        });
        let mut domains: Vec<DomainAttestation> = ident
            .domains
            .iter()
            .map(|d| DomainAttestation { domain: d.domain.clone(), attestor: d.attestor, expires: d.expires })
            .collect();
        domains.dedup();
        let r = self.restriction(a);
        let evidence = self.store.evidence(a);
        if let Some(p) = &evidence {
            let _ = self.s_fork_seq(&p.author());
        }
        let retired = cuts.retire.as_ref().map(|rec| Retired {
            seq: rec.seq(),
            successor: match &rec.body() {
                Some(Body::Retire(b)) => b.successor,
                _ => None,
            },
        });
        let log = self.store.log(a, 1, usize::MAX);
        let record_count = log.iter().filter(|r| self.rec(&r.id()).is_some_and(|x| self.active(&x))).count() as u64;
        let mut listing_roots = BTreeSet::new();
        for l in self.s_by_author_kind(a, names::LISTING) {
            listing_roots.insert(self.root_of(&l.id()));
        }
        let mut hidden_listing_count = 0;
        let mut product_keys = BTreeSet::new();
        for root in &listing_roots {
            let Some(c) = self.chain(root) else { continue };
            if c.latest.is_some() && self.display_status(&c) == Status::Hidden {
                hidden_listing_count += 1;
            }
            if c.reference().is_some_and(|x| self.active(x))
                && let Some(k) = reference_product(&c.base)
            {
                product_keys.insert(k.as_str().to_owned());
            }
        }
        let mut seller_reviews = SellerReviews { total: 0, with_valid_receipt: 0 };
        for k in &product_keys {
            for e in &self.review_set(k).evals {
                if e.excluded.is_none() {
                    seller_reviews.total += 1;
                    if self.purchase(&e.chain).receipt_seller == Some(*a) {
                        seller_reviews.with_valid_receipt += 1;
                    }
                }
            }
        }
        Some(AuthorView {
            id: *a,
            name,
            role,
            url,
            about,
            profile: profile_mod,
            profile_status,
            verified,
            domains,
            status: cuts.status(),
            restricted: r.restricted,
            restriction: (r.restricted || r.hidden_listings > 0)
                .then_some(RestrictionView { hidden_listings: r.hidden_listings, distinct_flaggers: r.distinct_flaggers }),
            revoked: cuts.revoke.map(|(v, attestor, _)| Revoked { attestor, valid_through_seq: v }),
            fork_seq: evidence.as_ref().map(|p| p.fork_seq()),
            fork_evidence: evidence.as_ref().map(|p| p.ids()),
            retired,
            seller_reviews,
            record_count,
            hidden_listing_count,
        })
    }

    /// Flags on the chain of `id` (7.3).
    pub fn flags(&self, id: &RecordId) -> Vec<FlagView> {
        let root = self.root_of(id);
        let fi = self.flag_info(&root);
        fi.entries
            .iter()
            .map(|e| {
                let l = e.chain.latest.as_ref().expect("latest");
                let (reason, note) = match (&l.body(), e.state) {
                    (Some(Body::Flag(b)), s) if s != FlagState::Unavailable => {
                        (Some(b.reason.as_str().to_owned()), b.note.clone())
                    }
                    _ => (None, None),
                };
                let a = e.chain.author();
                FlagView {
                    record: l.id(),
                    author: a,
                    author_name: self.display_name(&a),
                    author_verified: self.verified(&a),
                    reason,
                    note,
                    counted: e.excluded.is_none(),
                    excluded_reason: e.excluded,
                    ts: l.ts(),
                }
            })
            .collect()
    }

    /// Chains of an author (15.2).
    pub fn author_records(&self, a: &AuthorId, kinds: &[&str], limit: usize, from_seq: u64) -> Vec<ChainSummary> {
        let mut out = Vec::new();
        for r in self.store.log(a, from_seq.max(1), usize::MAX) {
            if out.len() >= limit {
                break;
            }
            if !kinds.is_empty() && !kinds.contains(&r.kind()) {
                continue;
            }
            if self.root_of(&r.id()) != r.id() {
                continue;
            }
            let Some(c) = self.chain(&r.id()) else { continue };
            let (latest, status) = match &c.latest {
                Some(l) => {
                    let s = if HIDEABLE.contains(&c.kind()) {
                        self.display_status(&c).as_str()
                    } else if c.retract.is_some() {
                        "retracted"
                    } else if !l.has_body() {
                        "unavailable"
                    } else {
                        "visible"
                    };
                    (l.clone(), s)
                }
                None => {
                    let Some(m) = c.base.members.iter().rev().find(|m| self.active(m)) else { continue };
                    (m.clone(), "invalid")
                }
            };
            let product = match c.kind() {
                names::PRODUCT => self.chain_product_key(&c.base),
                names::LISTING | names::REVIEW | names::OBSERVATION => {
                    reference_product(&c.base).map(|k| k.as_str().to_owned())
                }
                _ => None,
            };
            let title = match &latest.body() {
                Some(Body::Listing(l)) => l.title.clone(),
                Some(Body::Review(r)) => r.title.clone(),
                Some(Body::Product(p)) => Some(p.name.clone()),
                Some(Body::Profile(p)) => Some(p.name.clone()),
                _ => None,
            };
            out.push(ChainSummary {
                root: c.root(),
                latest: latest.id(),
                kind: c.kind().to_owned(),
                ts: latest.ts(),
                status,
                product,
                title,
            });
        }
        out
    }

    /// Validity and chain of one record (15.3).
    pub fn record_status(&self, id: &RecordId) -> Option<RecordStatus> {
        let r = self.rec(id)?;
        let v = self.validity(id);
        let (semantic_valid, reason) = match &v {
            Validity::Valid => (true, None),
            Validity::Invalid(s) => (false, Some(s.clone())),
            Validity::Unavailable => (false, Some("unavailable".to_owned())),
            Validity::Unknown => (false, Some("unknown_kind".to_owned())),
        };
        let root = self.root_of(id);
        let chain = self.chain(&root);
        let versions = chain.as_ref().map(|c| self.versions(c).iter().map(|m| m.id()).collect()).unwrap_or_default();
        let ignored = if semantic_valid {
            self.ignored_reason(&r).or_else(|| {
                let c = chain.as_ref()?;
                if c.retract.as_ref().is_some_and(|t| r.seq() > t.seq()) {
                    Some("retracted")
                } else if r.kind().starts_with("attest.") && r.seq() > self.policy.cap(&r.author()) {
                    Some("not_after_seq")
                } else {
                    None
                }
            })
        } else {
            None
        };
        Some(RecordStatus { semantic_valid, reason, root, versions, ignored })
    }

    fn all_records(&self) -> Vec<RecordId> {
        let mut out = Vec::new();
        for a in self.store.authors() {
            out.extend(self.store.log(&a, 1, usize::MAX).iter().map(|r| r.id()));
        }
        out
    }

    /// Store counts (15.4).
    pub fn stats(&self) -> Stats {
        let ids = self.all_records();
        let mut invalid = 0;
        let mut unknown = 0;
        for id in &ids {
            match self.validity(id) {
                Validity::Invalid(_) => invalid += 1,
                Validity::Unknown => unknown += 1,
                _ => {}
            }
        }
        let count_chains = |kind: &str| -> u64 {
            let mut roots = BTreeSet::new();
            for id in self.store.by_kind(kind) {
                roots.insert(self.root_of(&id));
            }
            roots.iter().filter(|r| self.chain_base(r).is_some_and(|b| b.reference.is_some())).count() as u64
        };
        let forked = self.store.forked_authors();
        Stats {
            records: (self.store.len() + 2 * forked.len()) as u64,
            authors: self.store.authors().len() as u64,
            products: self.product_chain_keys().len() as u64,
            listings: count_chains(names::LISTING),
            reviews: count_chains(names::REVIEW),
            forks: forked.len() as u64,
            invalid,
            redacted: self.store.bodyless_count() as u64,
            unknown,
        }
    }

    /// Every semantically invalid log record with its reason, sorted by ID.
    pub fn invalid_records(&self) -> Vec<(RecordId, String)> {
        let mut out: Vec<(RecordId, String)> = self
            .all_records()
            .into_iter()
            .filter_map(|id| match self.validity(&id) {
                Validity::Invalid(s) => Some((id, s)),
                _ => None,
            })
            .collect();
        out.sort();
        out
    }

    /// True if a tier-1 author's active record references `id` (section 15.5).
    pub fn referenced_by_tier1(&self, id: &RecordId) -> bool {
        let refs = self.s_by_target(id).into_iter().filter(|r| matches!(r.kind(), names::FLAG | names::APPEAL));
        let refs = refs.chain(self.s_referencing(id).into_iter().filter(|r| r.kind() == names::REVIEW));
        for r in refs {
            if self.is_tier1(&r.author()) && self.active(&r) {
                return true;
            }
        }
        false
    }

    fn is_tier1(&self, a: &AuthorId) -> bool {
        self.policy.attestor(a).is_some() || self.verified(a)
    }

    /// Admission tier of `author` under this context's policy (used as the
    /// admission policy).
    pub fn admission_tier(&self, a: &AuthorId) -> Tier {
        if self.is_tier1(a) {
            return Tier::One;
        }
        for r in self.store.log(a, 1, usize::MAX) {
            if self.referenced_by_tier1(&r.id()) {
                return Tier::Two;
            }
        }
        Tier::Three
    }
}
