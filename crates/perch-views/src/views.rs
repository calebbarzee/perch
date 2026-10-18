//! Listing, review, and product views (docs/views.md sections 8 to 10).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use perch_core::kinds::{Body, names};
use perch_core::{AuthorId, ProductKey, RecordId};
use perch_store::Store;

use crate::engine::{Chain, PersonId, Views, reference_product};
use crate::model::*;
use crate::num::mean;

/// Whether a listing feeds price figures (8.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Feed {
    Feeds,
    /// Meets rules 1-3; fails only the domain rule.
    UnverifiedDomain,
    No,
}

pub(crate) fn status_rank(s: Status) -> u8 {
    match s {
        Status::Visible => 0,
        Status::Hidden => 1,
        Status::Retracted => 2,
        Status::Unavailable => 3,
    }
}

impl<S: Store + ?Sized> Views<'_, S> {
    // --- Listings -------------------------------------------------------------

    /// `ListingView` for any member ID of a listing chain.
    pub fn listing(&self, id: &RecordId) -> Option<ListingView> {
        let c = self.chain_of(id)?;
        if c.kind() != names::LISTING || c.latest.is_none() {
            return None;
        }
        self.listing_view(&c)
    }

    pub(crate) fn feed(&self, c: &Chain) -> Feed {
        match self.price_excluded_reason(c) {
            None => Feed::Feeds,
            Some("domain_unverified") => Feed::UnverifiedDomain,
            Some(_) => Feed::No,
        }
    }

    /// The first rule of 8.3 that a listing fails, or `None` if it feeds prices.
    pub(crate) fn price_excluded_reason(&self, c: &Chain) -> Option<&'static str> {
        let status = self.display_status(c);
        if status != Status::Visible {
            return Some(status.as_str());
        }
        let Some(Body::Listing(l)) = c.latest_body() else { return Some("unavailable") };
        if !matches!(l.availability.as_str(), "in_stock" | "preorder") {
            return Some("availability");
        }
        if self.restricted(&c.author()) {
            return Some("seller_restricted");
        }
        if self.policy.price_requires_domain_verified && self.listing_domain(c).is_none() {
            return Some("domain_unverified");
        }
        None
    }

    /// `ListingView` of a chain, or `None` if it has no latest version or no
    /// known product (neither a root body nor a notice naming one).
    pub(crate) fn listing_view(&self, c: &Chain) -> Option<ListingView> {
        let latest = c.latest.as_ref()?;
        let product = reference_product(&c.base).cloned().or_else(|| self.stated_product(c))?;
        let seller = c.author();
        let body = match &latest.body() {
            Some(Body::Listing(l)) => Some(l),
            _ => None,
        };
        let domain = self.listing_domain(c);
        let price_excluded_reason = self.price_excluded_reason(c);
        let fi = self.flag_info(&c.root());
        let appeal = self.appeal(c);
        Some(ListingView {
            id: c.root(),
            latest: latest.id(),
            seller,
            seller_name: self.display_name(&seller),
            seller_verified: self.verified(&seller),
            seller_restricted: self.restricted(&seller),
            seller_status: self.status(&seller),
            domain_verified: domain.is_some(),
            feeds_prices: price_excluded_reason.is_none(),
            price_excluded_reason,
            domain: domain.map(|d| DomainAttestation { domain: d.domain, attestor: d.attestor, expires: d.expires }),
            product,
            title: body.and_then(|b| b.title.clone()),
            url: body.map(|b| b.url.clone()),
            price: body.map(|b| Price { amount: b.price.amount, currency: b.price.currency.clone() }),
            availability: body.map(|b| b.availability.as_str().to_owned()),
            condition: body.map(|b| b.condition.as_str().to_owned()),
            ships_to: body.and_then(|b| b.ships_to.clone()).unwrap_or_default(),
            location: body
                .and_then(|b| b.location.as_ref())
                .map(|l| Location { country: l.country.clone(), region: l.region.clone() }),
            updated_ts: latest.ts(),
            status: self.display_status(c),
            flags: fi.flags.clone(),
            appealed: appeal.is_some(),
            appeal,
            redaction: self.redaction(c),
            price_history: self.price_history(c),
        })
    }

    fn price_history(&self, c: &Chain) -> Vec<PricePoint> {
        let mut out = Vec::new();
        for v in &self.versions(c) {
            if let Some(Body::Listing(l)) = &v.body() {
                out.push(PricePoint {
                    ts: v.ts(),
                    amount: l.price.amount,
                    currency: l.price.currency.clone(),
                    source: "seller",
                    record: v.id(),
                });
            }
        }
        let product = reference_product(&c.base);
        let mut seen = BTreeSet::new();
        for m in &c.base.members {
            for o in self.s_referencing(&m.id()) {
                if o.kind() != names::OBSERVATION || !seen.insert(o.id()) {
                    continue;
                }
                let Some(Body::Observation(b)) = &o.body() else { continue };
                let Some(l) = b.listing else { continue };
                if self.root_of(&l) != c.root()
                    || Some(&b.product) != product
                    || !self.active(&o)
                    || !self.validity(&o.id()).is_valid()
                    || !self.verified(&o.author())
                {
                    continue;
                }
                out.push(PricePoint {
                    ts: b.observed_at,
                    amount: b.price.amount,
                    currency: b.price.currency.clone(),
                    source: "observation",
                    record: o.id(),
                });
            }
        }
        out.sort_by(|a, b| (a.ts, a.source != "seller", a.record).cmp(&(b.ts, b.source != "seller", b.record)));
        out
    }

    // --- Reviews ----------------------------------------------------------------

    pub(crate) fn review_view(&self, c: &Chain, excluded: Option<&'static str>) -> Option<ReviewView> {
        let latest = c.latest.as_ref()?;
        let author = c.author();
        let body = match &latest.body() {
            Some(Body::Review(r)) => Some(r),
            _ => None,
        };
        let purchase = if body.is_some() { self.purchase(c).outcome } else { "none" };
        let fi = self.flag_info(&c.root());
        Some(ReviewView {
            id: c.root(),
            latest: latest.id(),
            author,
            author_name: self.display_name(&author),
            author_verified: self.verified(&author),
            author_status: self.status(&author),
            verified_purchase: purchase == "verified",
            purchase,
            counted: excluded.is_none(),
            excluded_reason: excluded,
            rating: body.map(|b| b.rating),
            title: body.and_then(|b| b.title.clone()),
            text: body.map(|b| b.text.clone()),
            ts: latest.ts(),
            listing: body.and_then(|b| b.listing).map(|l| self.root_of(&l)),
            status: self.display_status(c),
            flags: fi.flags.clone(),
            appeal: self.appeal(c),
            redaction: self.redaction(c),
        })
    }

    /// Rating summary of a product (9.4). `with_purchase` computes the
    /// verified-purchase part (needs receipt signature checks).
    pub(crate) fn ratings(&self, key: &str, with_purchase: bool) -> RatingSummary {
        let set = self.review_set(key);
        let mut all = Acc::default();
        let mut vp = Acc::default();
        let mut vi = Acc::default();
        let mut ex = Excluded { revoked: set.revoked, ..Excluded::default() };
        for e in &set.evals {
            match e.excluded {
                Some("unavailable") => ex.unavailable += 1,
                Some("retracted") => ex.retracted += 1,
                Some("seller") => ex.seller += 1,
                Some("forked") => ex.forked += 1,
                Some("superseded") => ex.superseded += 1,
                Some(_) => {}
                None => {
                    let Some(Body::Review(r)) = e.chain.latest_body() else { continue };
                    all.add(r.rating);
                    if self.verified(&e.chain.author()) {
                        vi.add(r.rating);
                    }
                    if with_purchase && self.purchase(&e.chain).outcome == "verified" {
                        vp.add(r.rating);
                    }
                }
            }
        }
        let a = all.stats();
        RatingSummary {
            count: a.count,
            mean: a.mean,
            histogram: a.histogram,
            verified_purchase: vp.stats(),
            verified_identity: vi.stats(),
            excluded: ex,
        }
    }

    // --- Products ---------------------------------------------------------------

    /// Product chains used as sources, with moderation and exclusions (10.3).
    pub(crate) fn product_sources(&self, key: &str) -> ProductSources {
        let pr = self.product_records(key);
        self.product_sources_of(&pr.products)
    }

    /// Like [`Self::product_sources`], over given product chains of one key.
    pub(crate) fn product_sources_of(&self, chains: &[Arc<Chain>]) -> ProductSources {
        let mut ex = ExcludedSources::default();
        let mut by_status: BTreeMap<u8, Vec<Arc<Chain>>> = BTreeMap::new();
        for c in chains {
            if self.excluded_forked(&c.author()) {
                ex.forked += 1;
                continue;
            }
            if c.latest.is_none() {
                let cut = self.cuts(&c.author()).revoke_cut;
                if cut != u64::MAX && c.base.members.iter().filter(|m| m.has_body()).all(|m| m.seq() > cut) {
                    ex.revoked += 1;
                }
                continue;
            }
            let s = self.display_status(c);
            match s {
                Status::Retracted => ex.retracted += 1,
                Status::Unavailable => ex.unavailable += 1,
                _ => {}
            }
            by_status.entry(status_rank(s)).or_default().push(c.clone());
        }
        // Facts come from visible chains only (7.6).
        let visible = by_status.get(&0).cloned().unwrap_or_default();
        let hidden = by_status.get(&1).cloned().unwrap_or_default();
        ex.hidden = hidden.len() as u64;
        // Hiding applies per chain. The key is hidden only when it has
        // product chains and all of them are hidden (7.6).
        let total: usize = by_status.values().map(Vec::len).sum();
        let status = if !visible.is_empty() {
            Status::Visible
        } else if total > 0 && hidden.len() == total {
            Status::Hidden
        } else {
            [Status::Retracted, Status::Unavailable, Status::Hidden]
                .into_iter()
                .find(|s| by_status.contains_key(&status_rank(*s)))
                .unwrap_or(Status::Visible)
        };
        let sources = visible;
        let all: Vec<Arc<Chain>> = by_status.values().flatten().cloned().collect();
        ProductSources { sources, all, status, excluded: ex }
    }

    pub(crate) fn product_moderation(&self, ps: &ProductSources) -> Moderation {
        let mut authors = BTreeSet::new();
        let mut persons: BTreeSet<PersonId> = BTreeSet::new();
        let mut reasons: BTreeMap<String, u64> = BTreeMap::new();
        let mut appeal: Option<Appeal> = None;
        for c in &ps.all {
            let fi = self.flag_info(&c.root());
            for e in &fi.entries {
                let superseded = e.excluded == Some("superseded");
                if e.state == crate::engine::FlagState::Live && !superseded {
                    authors.insert(e.chain.author());
                    if let Some(Body::Flag(b)) = e.chain.latest_body() {
                        *reasons.entry(b.reason.as_str().to_owned()).or_default() += 1;
                    }
                }
            }
            persons.extend(fi.counted_persons.iter().cloned());
            if let Some(a) = self.appeal(c)
                && appeal.as_ref().is_none_or(|b| (a.ts, a.record) > (b.ts, b.record))
            {
                appeal = Some(a);
            }
        }
        let redaction = if ps.status == Status::Unavailable {
            ps.all.iter().find(|c| self.display_status(c) == Status::Unavailable).and_then(|c| self.redaction(c))
        } else {
            None
        };
        Moderation {
            status: ps.status,
            flags: Flags { count: authors.len() as u64, counted: persons.len() as u64, reasons },
            appeal,
            redaction,
        }
    }

    /// A fact over the source chains for one field (10.4).
    pub(crate) fn fact(&self, values: Vec<(String, AuthorId, RecordId)>) -> Option<Fact<String>> {
        if values.is_empty() {
            return None;
        }
        let mut groups: BTreeMap<String, Vec<(AuthorId, RecordId)>> = BTreeMap::new();
        for (v, a, r) in values {
            groups.entry(v).or_default().push((a, r));
        }
        // Person counts decide only between several values.
        let rank = groups.len() > 1;
        let mut ranked: Vec<(u64, u64, String, Vec<Source>)> = groups
            .into_iter()
            .map(|(v, list)| {
                let mut vp = BTreeSet::new();
                let mut ap = BTreeSet::new();
                let mut sources: Vec<Source> = list
                    .into_iter()
                    .map(|(author, record)| {
                        let verified = self.verified(&author);
                        if rank {
                            let p = self.person_id(&author);
                            if verified {
                                vp.insert(p.clone());
                            }
                            ap.insert(p);
                        }
                        Source { author, record, verified }
                    })
                    .collect();
                sources.sort_by(|x, y| (!x.verified, x.author, x.record).cmp(&(!y.verified, y.author, y.record)));
                sources.dedup();
                (vp.len() as u64, ap.len() as u64, v, sources)
            })
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
        let mut it = ranked.into_iter();
        let (_, _, value, sources) = it.next()?;
        Some(Fact { value, sources, conflicts: it.map(|(_, _, value, sources)| Conflict { value, sources }).collect() })
    }

    /// Name, brand, category, mpn facts of a product (shared by search).
    pub(crate) fn product_text(&self, ps: &ProductSources) -> ProductText {
        let mut name = Vec::new();
        let mut brand = Vec::new();
        let mut category = Vec::new();
        let mut mpn = Vec::new();
        for c in &ps.sources {
            let Some(l) = c.latest.as_ref() else { continue };
            if let Some(Body::Product(p)) = &l.body() {
                name.push((p.name.clone(), c.author(), l.id()));
                brand.push((p.brand.clone(), c.author(), l.id()));
                if let Some(x) = &p.category {
                    category.push((x.clone(), c.author(), l.id()));
                }
                if let Some(x) = &p.mpn {
                    mpn.push((x.clone(), c.author(), l.id()));
                }
            }
        }
        ProductText {
            name: self.fact(name).unwrap_or_else(empty_fact),
            brand: self.fact(brand).unwrap_or_else(empty_fact),
            category: self.fact(category),
            mpn: self.fact(mpn).map(|f| f.value),
        }
    }

    /// True if any product, listing, or review chain with a latest version has this key.
    pub(crate) fn product_exists(&self, key: &str) -> bool {
        let pr = self.product_records(key);
        pr.products.iter().chain(&pr.listings).chain(&pr.reviews).any(|c| c.latest.is_some())
    }

    /// `ProductView` (10.6).
    pub fn product(&self, key: &str) -> Option<ProductView> {
        let pkey = ProductKey::parse(key).ok()?;
        if !self.product_exists(key) {
            return None;
        }
        let ps = self.product_sources(key);
        let text = self.product_text(&ps);
        let mut description = Vec::new();
        let mut attrs: BTreeMap<String, Vec<(String, AuthorId, RecordId)>> = BTreeMap::new();
        let mut images: BTreeMap<(String, Option<String>), bool> = BTreeMap::new();
        for c in &ps.sources {
            let Some(l) = c.latest.as_ref() else { continue };
            let Some(Body::Product(p)) = &l.body() else { continue };
            if let Some(d) = &p.description {
                description.push((d.clone(), c.author(), l.id()));
            }
            for (k, v) in p.attributes.iter().flatten() {
                attrs.entry(k.clone()).or_default().push((v.clone(), c.author(), l.id()));
            }
            let verified = self.verified(&c.author());
            for im in p.images.iter().flatten() {
                let e = images.entry((im.url.clone(), im.sha256.map(|h| h.to_hex()))).or_insert(false);
                *e |= verified;
            }
        }
        let mut images: Vec<Image> =
            images.into_iter().map(|((url, sha256), author_verified)| Image { url, sha256, author_verified }).collect();
        images.sort_by(|a, b| (!a.author_verified, &a.url, &a.sha256).cmp(&(!b.author_verified, &b.url, &b.sha256)));

        let pr = self.product_records(key);
        let mut listings: Vec<ListingView> = pr.listings.iter().filter_map(|c| self.listing_view(c)).collect();
        // Listings that feed prices first, then the rest (8.4).
        listings.sort_by(|a, b| {
            let pa = a.price.as_ref().map(|p| (0, p.currency.clone(), p.amount));
            let pb = b.price.as_ref().map(|p| (0, p.currency.clone(), p.amount));
            (!a.feeds_prices, status_rank(a.status), pa.unwrap_or((1, String::new(), 0)), a.id)
                .cmp(&(!b.feeds_prices, status_rank(b.status), pb.unwrap_or((1, String::new(), 0)), b.id))
        });
        let set = self.review_set(key);
        let mut reviews: Vec<ReviewView> = set.evals.iter().filter_map(|e| self.review_view(&e.chain, e.excluded)).collect();
        reviews.sort_by(|a, b| b.ts.cmp(&a.ts).then(a.id.cmp(&b.id)));

        Some(ProductView {
            key: pkey.clone(),
            moderation: self.product_moderation(&ps),
            name: text.name,
            brand: text.brand,
            category: text.category,
            description: self.fact(description),
            gtin: pkey.gtin().map(str::to_owned),
            mpn: text.mpn,
            attributes: attrs.into_iter().filter_map(|(k, v)| self.fact(v).map(|f| (k, f))).collect(),
            images,
            listings,
            reviews,
            ratings: self.ratings(key, true),
            price_range: self.price_range(key),
            excluded_sources: ps.excluded,
        })
    }

    /// Price statistics per currency (10.5).
    pub(crate) fn price_range(&self, key: &str) -> BTreeMap<String, PriceStats> {
        let mut per: BTreeMap<String, (Vec<u64>, u64)> = BTreeMap::new();
        for c in &self.product_records(key).listings {
            let Some(Body::Listing(l)) = c.latest_body() else { continue };
            match self.feed(c) {
                Feed::Feeds => per.entry(l.price.currency.clone()).or_default().0.push(l.price.amount),
                Feed::UnverifiedDomain => per.entry(l.price.currency.clone()).or_default().1 += 1,
                Feed::No => {}
            }
        }
        per.into_iter()
            .map(|(cur, (mut v, ex))| {
                v.sort_unstable();
                let stats = PriceStats {
                    min: v.first().copied(),
                    max: v.last().copied(),
                    median: crate::num::lower_median(&v),
                    count: v.len() as u64,
                    excluded_unverified_domain: ex,
                };
                (cur, stats)
            })
            .collect()
    }

    /// Minimum feeding price, preferring `currency` (11.6).
    pub(crate) fn min_price(&self, key: &str, currency: Option<&str>) -> Option<Price> {
        let mut per: BTreeMap<String, (u64, u64)> = BTreeMap::new();
        for c in &self.product_records(key).listings {
            let Some(Body::Listing(l)) = c.latest_body() else { continue };
            if self.feed(c) == Feed::Feeds {
                let e = per.entry(l.price.currency.clone()).or_insert((0, u64::MAX));
                e.0 += 1;
                e.1 = e.1.min(l.price.amount);
            }
        }
        if let Some(cur) = currency
            && let Some((_, m)) = per.get(cur)
        {
            return Some(Price { amount: *m, currency: cur.to_owned() });
        }
        per.into_iter()
            .min_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(&b.0)))
            .map(|(currency, (_, amount))| Price { amount, currency })
    }
}

fn empty_fact() -> Fact<String> {
    Fact { value: String::new(), sources: Vec::new(), conflicts: Vec::new() }
}

pub(crate) struct ProductSources {
    pub sources: Vec<Arc<Chain>>,
    /// All non-forked product chains with a latest version.
    pub all: Vec<Arc<Chain>>,
    pub status: Status,
    pub excluded: ExcludedSources,
}

pub(crate) struct ProductText {
    pub name: Fact<String>,
    pub brand: Fact<String>,
    pub category: Option<Fact<String>>,
    pub mpn: Option<String>,
}



#[derive(Default)]
pub(crate) struct Acc {
    hist: [u64; 5],
    sum: u64,
    n: u64,
}

impl Acc {
    pub fn add(&mut self, rating: u8) {
        self.hist[usize::from(rating.clamp(1, 5)) - 1] += 1;
        self.sum += u64::from(rating);
        self.n += 1;
    }
    pub fn stats(&self) -> RatingStats {
        RatingStats { count: self.n, mean: mean(self.sum, self.n), histogram: self.hist }
    }
}
