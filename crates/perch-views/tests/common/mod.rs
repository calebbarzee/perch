//! A small world builder for scenario tests.
#![allow(dead_code)]

use perch_core::{ProductKey, Receipt, Record, RecordId};
use perch_demo::Author;
use perch_store::{Insert, MemoryStore, Store};
use perch_views::{Policy, Views, default_policy};
use serde_json::{Value, json};

pub const BASE: u64 = 1_767_225_600_000;
pub const DAY: u64 = 86_400_000;
pub const NOW: u64 = BASE + 100 * DAY;
pub const EXP: u64 = BASE + 1000 * DAY;

pub struct World {
    pub store: MemoryStore,
    pub attestor: Author,
    pub policy: Policy,
    clock: u64,
    n: u64,
}

impl World {
    pub fn new() -> World {
        let attestor = Author::from_label("test/attestor");
        let policy = default_policy(NOW, &[attestor.id()]);
        World { store: MemoryStore::new(), attestor, policy, clock: BASE, n: 0 }
    }

    pub fn views(&self) -> Views<'_, MemoryStore> {
        Views::new(&self.store, &self.policy)
    }

    pub fn key(&mut self, label: &str) -> Author {
        self.n += 1;
        Author::from_label(&format!("test/{label}/{}", self.n))
    }

    pub fn tick(&mut self) -> u64 {
        self.clock += 60_000;
        self.clock
    }

    pub fn insert(&mut self, r: &Record) {
        let out = self.store.insert(r.clone());
        assert!(matches!(out, Insert::Inserted | Insert::Fork { .. }), "insert: {out:?}");
    }

    pub fn put(&mut self, a: &mut Author, kind: &str, body: Value) -> Record {
        self.put_r(a, kind, body, None)
    }

    pub fn put_r(&mut self, a: &mut Author, kind: &str, body: Value, replaces: Option<RecordId>) -> Record {
        let ts = self.tick();
        let r = a.publish(ts, kind, body, replaces);
        self.insert(&r);
        r
    }

    /// Attestor publishes `attest.identity` for `subject` with person pseudonym `person`.
    pub fn verify(&mut self, subject: &Author, person: &str) -> Record {
        let mut att = self.attestor.clone();
        let r = self.put(
            &mut att,
            "attest.identity",
            json!({ "subject": subject.id().to_hex(), "person": hex_person(person), "level": "person", "expires": EXP }),
        );
        self.attestor = att;
        r
    }

    pub fn attest(&mut self, att: &mut Author, subject: &Author, person: &str, expires: u64) -> Record {
        self.put(
            att,
            "attest.identity",
            json!({ "subject": subject.id().to_hex(), "person": hex_person(person), "level": "person", "expires": expires }),
        )
    }

    pub fn domain(&mut self, subject: &Author, domain: &str) -> Record {
        let mut att = self.attestor.clone();
        let r = self.put(&mut att, "attest.domain", json!({ "subject": subject.id().to_hex(), "domain": domain, "expires": EXP }));
        self.attestor = att;
        r
    }

    pub fn product(&mut self, a: &mut Author, name: &str, gtin: &str) -> ProductKey {
        self.put(a, "product", json!({ "name": name, "brand": "Brand", "gtin": gtin }));
        ProductKey::from_gtin(gtin).unwrap()
    }

    pub fn listing(&mut self, seller: &mut Author, key: &ProductKey, url: &str, amount: u64) -> Record {
        self.put(seller, "listing", listing_body(key, url, amount))
    }

    pub fn review(&mut self, a: &mut Author, key: &ProductKey, rating: u8, receipt: Option<&Receipt>) -> Record {
        let mut b = json!({ "product": key.as_str(), "rating": rating, "text": "Text." });
        if let Some(rc) = receipt {
            b["receipt"] = rc.to_value();
        }
        self.put(a, "review", b)
    }

    pub fn flag(&mut self, a: &mut Author, target: RecordId) -> Record {
        self.put(a, "flag", json!({ "target": target.to_hex(), "reason": "fraud" }))
    }

    pub fn retract(&mut self, a: &mut Author, target: RecordId) -> Record {
        self.put(a, "retract", json!({ "target": target.to_hex() }))
    }

    /// `n` verified flaggers, each its own person.
    pub fn flaggers(&mut self, n: usize, tag: &str) -> Vec<Author> {
        (0..n)
            .map(|i| {
                let a = self.key(&format!("{tag}{i}"));
                self.verify(&a, &format!("{tag}-person-{i}"));
                a
            })
            .collect()
    }
}

pub fn listing_body(key: &ProductKey, url: &str, amount: u64) -> Value {
    json!({ "product": key.as_str(), "url": url, "price": { "amount": amount, "currency": "EUR" },
            "availability": "in_stock", "condition": "new" })
}

pub fn hex_person(p: &str) -> String {
    perch_core::ids::encode_hex(&perch_core::sha256(p.as_bytes()))
}

pub fn receipt(seller: &Author, listing_root: RecordId, buyer: &Author) -> Receipt {
    perch_core::sign_receipt(seller.keypair(), listing_root, buyer.id(), BASE, [7; 32]).unwrap()
}

pub const G1: &str = "4006381333931";
pub const G2: &str = "4006381333948";
