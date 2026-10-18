//! Scenario helpers: attestors, verified people, sellers, products, listings.

use crate::fixture::{Log, Record, T0, gtin_key, policy};
use serde_json::Value;

/// Time at which tests evaluate views.
pub const NOW: i64 = T0 + 1_000_000_000;
/// Default attestation expiry: well after `NOW`.
pub const EXPIRES: i64 = T0 + 10_000_000_000;

/// A GTIN-13 with a valid check digit, and its product key.
pub const GTIN_A: &str = "4006381333931";
pub const GTIN_B: &str = "5901234123457";

pub fn key_a() -> String {
    gtin_key(GTIN_A)
}
pub fn key_b() -> String {
    gtin_key(GTIN_B)
}

/// A small world with one identity attestor and one domain attestor.
pub struct World {
    pub ida: Log,
    pub doma: Log,
    pub logs: Vec<Log>,
}

impl World {
    pub fn new() -> World {
        let mut ida = Log::new("identity-attestor");
        ida.profile("Identity Attestor", "attestor");
        let mut doma = Log::new("domain-attestor");
        doma.profile("Domain Attestor", "attestor");
        World { ida, doma, logs: Vec::new() }
    }

    /// Default policy trusting this world's attestors.
    pub fn policy(&self) -> Value {
        policy(NOW, &[&self.ida.author()], &[&self.doma.author()])
    }

    /// A new author verified by the identity attestor as `person`.
    pub fn verified(&mut self, label: &str, person: &str) -> Log {
        let l = Log::new(label);
        self.ida.attest_identity(&l.author(), person, EXPIRES);
        l
    }

    /// `n` verified people with distinct persons, labelled `{prefix}{i}`.
    pub fn crowd(&mut self, prefix: &str, n: usize) -> Vec<Log> {
        (0..n).map(|i| {
            let label = format!("{prefix}{i}");
            self.verified(&label, &label)
        }).collect()
    }

    /// A verified, domain-verified seller with a profile.
    pub fn seller(&mut self, label: &str, domain: &str) -> Log {
        let mut s = self.verified(label, label);
        s.profile(label, "seller");
        self.doma.attest_domain(&s.author(), domain, EXPIRES);
        s
    }

    /// Every record of the attestors, the extra logs, and the given logs, in per-author order.
    pub fn records(&self, logs: &[&Log]) -> Vec<Record> {
        let mut out = Vec::new();
        out.extend(self.ida.records.iter().cloned());
        out.extend(self.doma.records.iter().cloned());
        for l in &self.logs {
            out.extend(l.records.iter().cloned());
        }
        for l in logs {
            out.extend(l.records.iter().cloned());
        }
        out
    }

    pub fn lines(&self, logs: &[&Log]) -> Vec<String> {
        self.records(logs).iter().map(|r| r.wire()).collect()
    }
}

impl Default for World {
    fn default() -> Self {
        World::new()
    }
}

/// Product A created by `author`, and a listing for it by `seller` at `amount` minor units.
pub fn product_and_listing(maker: &mut Log, seller: &mut Log, domain: &str, amount: i64) -> (Record, Record) {
    let p = maker.product_gtin("Widget", "Acme", GTIN_A);
    let l = seller.listing(&key_a(), &format!("https://{domain}/widget"), amount);
    (p, l)
}
