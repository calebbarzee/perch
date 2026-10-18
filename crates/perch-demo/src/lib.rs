//! # perch-demo
//!
//! Deterministic demo and test dataset generator for Perch.
//!
//! [`generate`] returns signed records. The same `(seed, size)` gives the
//! same records, byte for byte, on every platform: keys derive from labels
//! (`sha256("perch-demo/v1/<seed>/<label>")`), times derive from
//! [`BASE_TS`], and randomness comes from a seeded integer generator.
//! Ed25519 signatures are deterministic ([RFC 8032](https://www.rfc-editor.org/rfc/rfc8032)).
//!
//! The demo secret keys are public by construction. Never use them outside
//! demos and tests.
//!
//! [`generate_demo`] also returns named actors, products, listings, and
//! reviews of the fixed scenario, for tests and the UI. Scenario contents
//! are documented in [`scenario`].
//!
//! Datasets from different seeds use different keys, so they never fork
//! each other's logs when merged.

#![warn(missing_docs)]

pub mod author;
pub mod gtin;
pub mod rng;
pub mod scenario;
mod words;

mod bulk;

use std::collections::BTreeMap;

use perch_core::serde_json::{Map, Value, json};
use perch_core::{AuthorId, ProductKey, Receipt, Record, RecordId};

pub use author::{Author, keypair_for_label};
use rng::Rng;

/// 2026-01-01T00:00:00Z in Unix milliseconds. All demo times derive from it.
pub const BASE_TS: u64 = 1_767_225_600_000;
/// One day in milliseconds.
pub const DAY: u64 = 86_400_000;
/// The `policy.now` the demo is designed for: [`BASE_TS`] + 270 days
/// (2026-09-28T00:00:00Z). Every demo record except the scenario's
/// future-dated listing has a lower `ts`.
pub const DEMO_NOW: u64 = BASE_TS + 270 * DAY;
/// Default seed, used by `perch-node demo` and the web app.
pub const DEMO_SEED: u64 = 1;

/// Dataset size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Size {
    /// About 300 records: the fixed scenario plus a few generated products.
    Small,
    /// About 5,000 records.
    Medium,
    /// About 100,000 records.
    Large,
}

impl Size {
    /// Approximate target record count.
    pub fn target(self) -> usize {
        match self {
            Size::Small => 300,
            Size::Medium => 5_000,
            Size::Large => 100_000,
        }
    }

    /// Parses `small`, `medium`, or `large`.
    pub fn parse(s: &str) -> Option<Size> {
        match s {
            "small" => Some(Size::Small),
            "medium" => Some(Size::Medium),
            "large" => Some(Size::Large),
            _ => None,
        }
    }
}

/// A generated dataset with the names of its scenario parts.
#[derive(Debug, Clone)]
pub struct Demo {
    /// All records, in generation order. Each author's records appear in
    /// seq order. Includes the fork's second record (right after its twin),
    /// header-only records (bodies removed), and records of unknown kinds.
    pub records: Vec<Record>,
    /// Suggested `policy.now`: [`DEMO_NOW`].
    pub now: u64,
    /// The two demo attestors, in the order A, B. A default policy trusts
    /// both for `attest.identity`, `attest.domain`, and `attest.revoke`.
    pub attestors: Vec<AuthorId>,
    /// Scenario actors by name, for example `"seller_good"`.
    pub actors: BTreeMap<String, AuthorId>,
    /// Scenario products by name: product key.
    pub products: BTreeMap<String, ProductKey>,
    /// Scenario listings by name: root ID.
    pub listings: BTreeMap<String, RecordId>,
    /// Scenario reviews by name: root ID.
    pub reviews: BTreeMap<String, RecordId>,
    /// Other named scenario records (flags, attestations, invalid records,
    /// unknown kinds): record ID.
    pub records_by_name: BTreeMap<String, RecordId>,
    /// The fork evidence pair of the forked author, as two records.
    pub fork_evidence: [Record; 2],
    /// Attestor B signed honestly up to this seq. Its later records attest
    /// two sybil keys (a compromise). Set `not_after_seq` to this value in a
    /// policy to cut the compromise off.
    pub attestor_b_honest_through_seq: u64,
}

impl Demo {
    /// The dataset as JSON Lines (one canonical record per line).
    pub fn to_jsonl(&self) -> String {
        perch_core::to_jsonl(self.records.iter())
    }

    /// A JSON manifest of the names: `{ seed_now, attestors, actors,
    /// products, listings, reviews, records, fork_evidence,
    /// attestor_b_honest_through_seq }`. Keys are sorted.
    pub fn manifest(&self) -> Value {
        let hexmap = |m: &BTreeMap<String, String>| -> Value {
            Value::Object(m.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect::<Map<_, _>>())
        };
        let s = |m: &BTreeMap<String, RecordId>| -> BTreeMap<String, String> {
            m.iter().map(|(k, v)| (k.clone(), v.to_hex())).collect()
        };
        json!({
            "now": self.now,
            "attestors": self.attestors.iter().map(|a| a.to_hex()).collect::<Vec<_>>(),
            "actors": hexmap(&self.actors.iter().map(|(k, v)| (k.clone(), v.to_hex())).collect()),
            "products": hexmap(&self.products.iter().map(|(k, v)| (k.clone(), v.as_str().to_owned())).collect()),
            "listings": hexmap(&s(&self.listings)),
            "reviews": hexmap(&s(&self.reviews)),
            "records": hexmap(&s(&self.records_by_name)),
            "fork_evidence": [self.fork_evidence[0].id().to_hex(), self.fork_evidence[1].id().to_hex()],
            "attestor_b_honest_through_seq": self.attestor_b_honest_through_seq,
            "records_total": self.records.len(),
        })
    }

    /// Author ID of a scenario actor.
    ///
    /// # Panics
    /// If `name` is not a scenario actor.
    pub fn actor(&self, name: &str) -> AuthorId {
        *self.actors.get(name).unwrap_or_else(|| panic!("no demo actor {name:?}"))
    }

    /// Product key of a scenario product.
    ///
    /// # Panics
    /// If `name` is not a scenario product.
    pub fn product(&self, name: &str) -> &ProductKey {
        self.products.get(name).unwrap_or_else(|| panic!("no demo product {name:?}"))
    }

    /// Root ID of a scenario listing.
    ///
    /// # Panics
    /// If `name` is not a scenario listing.
    pub fn listing(&self, name: &str) -> RecordId {
        *self.listings.get(name).unwrap_or_else(|| panic!("no demo listing {name:?}"))
    }

    /// Root ID of a scenario review.
    ///
    /// # Panics
    /// If `name` is not a scenario review.
    pub fn review(&self, name: &str) -> RecordId {
        *self.reviews.get(name).unwrap_or_else(|| panic!("no demo review {name:?}"))
    }

    /// ID of another named scenario record.
    ///
    /// # Panics
    /// If `name` is unknown.
    pub fn record(&self, name: &str) -> RecordId {
        *self.records_by_name.get(name).unwrap_or_else(|| panic!("no demo record {name:?}"))
    }
}

/// Generates the dataset's records. Same as `generate_demo(seed, size).records`.
pub fn generate(seed: u64, size: Size) -> Vec<Record> {
    generate_demo(seed, size).records
}

/// Generates the dataset with its names.
pub fn generate_demo(seed: u64, size: Size) -> Demo {
    let mut g = Gen::new(seed);
    scenario::plan(&mut g, size);
    bulk::plan(&mut g, size);
    g.run();
    g.finish()
}

// ---------------------------------------------------------------------------
// Generator context
// ---------------------------------------------------------------------------

/// Index of an author in [`Gen`].
pub(crate) type Ax = usize;

type Action = Box<dyn FnOnce(&mut Gen)>;

/// State of a listing chain during generation.
#[derive(Debug, Clone)]
pub(crate) struct ListingInfo {
    pub seller: Ax,
    pub root: RecordId,
    pub root_ts: u64,
    pub latest: RecordId,
    pub product: ProductKey,
    pub url: String,
    pub amount: u64,
    pub currency: String,
    pub availability: &'static str,
    pub condition: &'static str,
    pub title: Option<String>,
}

impl ListingInfo {
    pub fn body(&self) -> Value {
        let mut b = json!({
            "product": self.product.as_str(),
            "url": self.url,
            "price": { "amount": self.amount, "currency": self.currency },
            "availability": self.availability,
            "condition": self.condition,
        });
        if let Some(t) = &self.title {
            b["title"] = Value::String(t.clone());
        }
        b
    }
}

/// The generator: authors, the event plan, and registries of named parts.
pub(crate) struct Gen {
    pub seed: u64,
    pub rng: Rng,
    authors: Vec<Author>,
    by_label: BTreeMap<String, Ax>,
    out: Vec<Record>,
    plan: BTreeMap<(u64, u64), Action>,
    plan_n: u64,
    cur_ts: u64,
    pub products: BTreeMap<String, ProductKey>,
    pub listings: BTreeMap<String, ListingInfo>,
    pub reviews: BTreeMap<String, Record>,
    pub named: BTreeMap<String, Record>,
    pub receipts: BTreeMap<String, Receipt>,
    pub fork_evidence: Option<[Record; 2]>,
    pub attestor_b_honest_through_seq: u64,
    pub(crate) bulk: bulk::State,
}

impl Gen {
    fn new(seed: u64) -> Gen {
        Gen {
            seed,
            rng: Rng::new(seed, "gen"),
            authors: Vec::new(),
            by_label: BTreeMap::new(),
            out: Vec::new(),
            plan: BTreeMap::new(),
            plan_n: 0,
            cur_ts: BASE_TS,
            products: BTreeMap::new(),
            listings: BTreeMap::new(),
            reviews: BTreeMap::new(),
            named: BTreeMap::new(),
            receipts: BTreeMap::new(),
            fork_evidence: None,
            attestor_b_honest_through_seq: 0,
            bulk: bulk::State::default(),
        }
    }

    /// The author for `label`, created on first use.
    pub fn ax(&mut self, label: &str) -> Ax {
        if let Some(&i) = self.by_label.get(label) {
            return i;
        }
        let i = self.authors.len();
        self.authors.push(Author::from_label(&format!("{}/{label}", self.seed)));
        self.by_label.insert(label.to_owned(), i);
        i
    }

    pub fn id(&self, a: Ax) -> AuthorId {
        self.authors[a].id()
    }

    pub fn author(&self, a: Ax) -> &Author {
        &self.authors[a]
    }

    /// A stable pseudonym for a human or business, as attestor `attestor`
    /// would publish it.
    pub fn person(&self, attestor: &str, human: &str) -> String {
        perch_core::ids::encode_hex(&perch_core::sha256(
            format!("perch-demo/v1/{}/person/{attestor}/{human}", self.seed).as_bytes(),
        ))
    }

    /// Schedules `f` at `day` days plus `minute` minutes after [`BASE_TS`].
    pub fn at(&mut self, day: u64, minute: u64, f: impl FnOnce(&mut Gen) + 'static) {
        self.at_ts(BASE_TS + day * DAY + minute * 60_000, f);
    }

    /// Schedules `f` at an absolute time.
    pub fn at_ts(&mut self, ts: u64, f: impl FnOnce(&mut Gen) + 'static) {
        self.plan_n += 1;
        self.plan.insert((ts, self.plan_n), Box::new(f));
    }

    /// The time of the event being run.
    pub fn now(&self) -> u64 {
        self.cur_ts
    }

    fn run(&mut self) {
        while let Some(((ts, _), f)) = self.plan.pop_first() {
            self.cur_ts = self.cur_ts.max(ts);
            f(self);
        }
    }

    /// Signs and appends a record by `a` at the current time (+1 ms per record).
    pub fn emit(&mut self, a: Ax, kind: &str, body: Value, replaces: Option<RecordId>) -> Record {
        let r = self.authors[a].publish(self.cur_ts, kind, body, replaces);
        self.cur_ts = self.cur_ts.max(r.ts()) + 1;
        self.out.push(r.clone());
        r
    }

    /// Signs a record at an explicit `ts` (for future-dated records).
    pub fn emit_at(&mut self, a: Ax, ts: u64, kind: &str, body: Value) -> Record {
        let r = self.authors[a].publish(ts, kind, body, None);
        self.out.push(r.clone());
        r
    }

    /// Appends a conflicting twin of `a`'s last record (fork evidence).
    pub fn emit_fork(&mut self, a: Ax, kind: &str, body: Value) -> Record {
        let r = self.authors[a].publish_conflicting(self.cur_ts, kind, body, None);
        self.cur_ts += 1;
        self.out.push(r.clone());
        r
    }

    /// Replaces the stored copy of record `id` with its header-only form,
    /// as a node holds a record whose body it removed.
    pub fn strip_body(&mut self, id: RecordId) {
        for r in self.out.iter_mut().rev() {
            if r.id() == id {
                *r = r.without_body();
                return;
            }
        }
        panic!("strip_body: record not found");
    }

    pub fn name(&mut self, name: &str, r: &Record) {
        self.named.insert(name.to_owned(), r.clone());
    }

    pub fn named(&self, name: &str) -> &Record {
        self.named.get(name).unwrap_or_else(|| panic!("no named record {name:?}"))
    }

    // --- Typed helpers ---------------------------------------------------

    pub fn profile(&mut self, a: Ax, name: &str, role: &str, url: Option<&str>) -> Record {
        let mut b = json!({ "name": name, "role": role });
        if let Some(u) = url {
            b["url"] = Value::String(u.to_owned());
        }
        self.emit(a, "profile", b, None)
    }

    /// `attest.identity` by attestor `att` for `subject`, with the person
    /// pseudonym of `human`.
    #[allow(clippy::too_many_arguments)]
    pub fn attest_identity(
        &mut self,
        att: Ax,
        att_label: &str,
        subject: Ax,
        human: &str,
        business: Option<&str>,
        country: &str,
        expires: u64,
    ) -> Record {
        let mut b = json!({
            "subject": self.id(subject).to_hex(),
            "person": self.person(att_label, human),
            "level": if business.is_some() { "business" } else { "person" },
            "country": country,
            "expires": expires,
        });
        if let Some(n) = business {
            b["legal_name"] = Value::String(n.to_owned());
        }
        self.emit(att, "attest.identity", b, None)
    }

    pub fn attest_domain(&mut self, att: Ax, subject: Ax, domain: &str, expires: u64) -> Record {
        let b = json!({ "subject": self.id(subject).to_hex(), "domain": domain, "expires": expires });
        self.emit(att, "attest.domain", b, None)
    }

    /// Publishes a product record and registers `name` → key (first record wins).
    pub fn product(&mut self, name: &str, a: Ax, body: Value, replaces: Option<RecordId>) -> Record {
        let r = self.emit(a, "product", body.clone(), replaces);
        let root = match replaces {
            None => r.id(),
            Some(_) => {
                return r;
            }
        };
        let key = perch_core::kinds::derive_product_key(r.body().expect("body"), root)
            .unwrap_or_else(|e| panic!("demo product {name:?} invalid: {e:?}"));
        self.products.entry(name.to_owned()).or_insert(key);
        r
    }

    pub fn key(&self, product: &str) -> ProductKey {
        self.products.get(product).unwrap_or_else(|| panic!("no product {product:?}")).clone()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn listing(
        &mut self,
        name: &str,
        seller: Ax,
        product: &str,
        url: &str,
        amount: u64,
        currency: &str,
        title: Option<&str>,
    ) -> Record {
        let mut info = ListingInfo {
            seller,
            root: RecordId([0; 32]),
            root_ts: 0,
            latest: RecordId([0; 32]),
            product: self.key(product),
            url: url.to_owned(),
            amount,
            currency: currency.to_owned(),
            availability: "in_stock",
            condition: "new",
            title: title.map(str::to_owned),
        };
        let r = self.emit(seller, "listing", info.body(), None);
        info.root = r.id();
        info.root_ts = r.ts();
        info.latest = r.id();
        self.listings.insert(name.to_owned(), info);
        r
    }

    /// A new listing version with a new price and availability.
    pub fn reprice(&mut self, name: &str, amount: u64, availability: &'static str) -> Record {
        let mut info = self.listings.get(name).unwrap_or_else(|| panic!("no listing {name:?}")).clone();
        info.amount = amount;
        info.availability = availability;
        let r = self.emit(info.seller, "listing", info.body(), Some(info.latest));
        info.latest = r.id();
        self.listings.insert(name.to_owned(), info);
        r
    }

    pub fn listing_info(&self, name: &str) -> &ListingInfo {
        self.listings.get(name).unwrap_or_else(|| panic!("no listing {name:?}"))
    }

    /// A receipt signed by the listing's seller for `buyer`.
    pub fn receipt(&mut self, listing: &str, buyer: Ax, purchased_at: u64) -> Receipt {
        let info = self.listing_info(listing).clone();
        let nonce = self.rng.bytes32();
        let buyer_id = self.id(buyer);
        perch_core::sign_receipt(self.authors[info.seller].keypair(), info.root, buyer_id, purchased_at, nonce)
            .expect("receipt")
    }

    /// Publishes a review and registers it under `name` (unless empty).
    #[allow(clippy::too_many_arguments)]
    pub fn review(
        &mut self,
        name: &str,
        a: Ax,
        product: &str,
        rating: u8,
        title: Option<&str>,
        text: &str,
        listing: Option<&str>,
        receipt: Option<&Receipt>,
    ) -> Record {
        let key = self.key(product);
        self.review_key(name, a, &key, rating, title, text, listing, receipt, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn review_key(
        &mut self,
        name: &str,
        a: Ax,
        key: &ProductKey,
        rating: u8,
        title: Option<&str>,
        text: &str,
        listing: Option<&str>,
        receipt: Option<&Receipt>,
        replaces: Option<RecordId>,
    ) -> Record {
        let mut b = json!({ "product": key.as_str(), "rating": rating, "text": text });
        if let Some(t) = title {
            b["title"] = Value::String(t.to_owned());
        }
        if let Some(l) = listing {
            b["listing"] = Value::String(self.listing_info(l).root.to_hex());
        }
        if let Some(rc) = receipt {
            b["receipt"] = rc.to_value();
        }
        let r = self.emit(a, "review", b, replaces);
        if !name.is_empty() {
            self.reviews.entry(name.to_owned()).or_insert_with(|| r.clone());
        }
        r
    }

    pub fn flag(&mut self, a: Ax, target: RecordId, reason: &str, note: Option<&str>) -> Record {
        let mut b = json!({ "target": target.to_hex(), "reason": reason });
        if let Some(n) = note {
            b["note"] = Value::String(n.to_owned());
        }
        self.emit(a, "flag", b, None)
    }

    pub fn retract(&mut self, a: Ax, target: RecordId) -> Record {
        self.emit(a, "retract", json!({ "target": target.to_hex() }), None)
    }

    pub fn observe(&mut self, crawler: Ax, listing: &str, amount: u64, observed_at: u64) -> Record {
        let info = self.listing_info(listing).clone();
        let b = json!({
            "product": info.product.as_str(),
            "url": info.url,
            "price": { "amount": amount, "currency": info.currency },
            "observed_at": observed_at.min(self.cur_ts),
            "listing": info.root.to_hex(),
            "availability": info.availability,
        });
        self.emit(crawler, "observation", b, None)
    }

    fn finish(self) -> Demo {
        let public = |k: &String| !k.starts_with('~');
        let actors = self.by_label.iter().filter(|(k, _)| public(k)).map(|(k, &i)| (k.clone(), self.authors[i].id())).collect();
        let products = self.products.iter().filter(|(k, _)| public(k)).map(|(k, v)| (k.clone(), v.clone())).collect();
        let listings = self.listings.iter().filter(|(k, _)| public(k)).map(|(k, v)| (k.clone(), v.root)).collect();
        let reviews = self.reviews.iter().filter(|(k, _)| public(k)).map(|(k, v)| (k.clone(), v.id())).collect();
        let records_by_name = self.named.iter().filter(|(k, _)| public(k)).map(|(k, v)| (k.clone(), v.id())).collect();
        let attestors = vec![self.authors[self.by_label["attestor_a"]].id(), self.authors[self.by_label["attestor_b"]].id()];
        Demo {
            records: self.out,
            now: DEMO_NOW,
            attestors,
            actors,
            products,
            listings,
            reviews,
            records_by_name,
            fork_evidence: self.fork_evidence.expect("scenario creates a fork"),
            attestor_b_honest_through_seq: self.attestor_b_honest_through_seq,
        }
    }
}

/// Serializes a value to a canonical JSON string (for tests and tools).
pub fn canonical(v: &Value) -> String {
    perch_core::to_canonical_json(v).expect("canonical")
}
