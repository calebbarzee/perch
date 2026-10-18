//! The [`Policy`] type (ARCHITECTURE 7.1, docs/views.md section 2).

use std::collections::BTreeMap;

use perch_core::AuthorId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Attestation kinds a policy can accept from an attestor.
pub const ATTESTOR_KINDS: [&str; 3] = ["attest.domain", "attest.identity", "attest.revoke"];

/// One trusted attestor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedAttestor {
    /// The attestor's key.
    pub author: AuthorId,
    /// Accepted kinds: a non-empty subset of [`ATTESTOR_KINDS`].
    pub kinds: Vec<String>,
    /// If set, only the attestor's records with `seq ≤ not_after_seq` count.
    #[serde(default)]
    pub not_after_seq: Option<u64>,
}

/// The inputs a user controls (ARCHITECTURE 7.1). Every view result is
/// reproducible from (records, policy).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    /// The time at which every time-dependent rule is evaluated, Unix ms.
    pub now: u64,
    /// Attestors and the kinds accepted from each.
    pub trusted_attestors: Vec<TrustedAttestor>,
    /// Flags needed to hide a record, for kinds not in `flag_thresholds`.
    pub flag_threshold: u64,
    /// Per-kind thresholds.
    #[serde(default)]
    pub flag_thresholds: BTreeMap<String, u64>,
    /// Only flags from verified authors count.
    pub flags_require_verified: bool,
    /// Hidden listings that restrict a seller person.
    pub seller_hidden_listing_limit: u64,
    /// Distinct counted flaggers needed to restrict a seller person.
    pub seller_min_distinct_flaggers: u64,
    /// Verified purchase also requires a verified review author.
    pub verified_purchase_requires_verified_buyer: bool,
    /// Only domain-verified listings feed price figures.
    pub price_requires_domain_verified: bool,
    /// Records with `ts > now + future_ts_tolerance_ms` are treated as not yet existing.
    pub future_ts_tolerance_ms: u64,
}

/// Why a policy is invalid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolicyError {
    /// The JSON does not have the policy shape.
    #[error("policy JSON: {0}")]
    Json(String),
    /// A field value breaks a rule in docs/views.md section 2.1.
    #[error("policy field {field}: {problem}")]
    Field {
        /// Field name.
        field: &'static str,
        /// What is wrong.
        problem: &'static str,
    },
}

const MAX_SAFE: u64 = (1 << 53) - 1;

impl Policy {
    /// Parses and validates a policy from JSON text.
    pub fn from_json(text: &str) -> Result<Policy, PolicyError> {
        let p: Policy = serde_json::from_str(text).map_err(|e| PolicyError::Json(e.to_string()))?;
        p.validate()?;
        Ok(p)
    }

    /// Parses and validates a policy from a JSON value.
    pub fn from_value(v: Value) -> Result<Policy, PolicyError> {
        let p: Policy = serde_json::from_value(v).map_err(|e| PolicyError::Json(e.to_string()))?;
        p.validate()?;
        Ok(p)
    }

    /// Checks the rules of docs/views.md section 2.1.
    pub fn validate(&self) -> Result<(), PolicyError> {
        let f = |field, problem| Err(PolicyError::Field { field, problem });
        // Every integer must be a JSON-safe integer (4.2), so the policy hash
        // is defined and JavaScript reads the same value.
        const BIG: &str = "must be at most 2^53 - 1";
        let ints: [(&'static str, u64); 5] = [
            ("now", self.now),
            ("future_ts_tolerance_ms", self.future_ts_tolerance_ms),
            ("flag_threshold", self.flag_threshold),
            ("seller_hidden_listing_limit", self.seller_hidden_listing_limit),
            ("seller_min_distinct_flaggers", self.seller_min_distinct_flaggers),
        ];
        for (field, v) in ints {
            if v > MAX_SAFE {
                return f(field, BIG);
            }
        }
        if self.flag_thresholds.values().any(|&t| t > MAX_SAFE) {
            return f("flag_thresholds", BIG);
        }
        if self.trusted_attestors.iter().any(|a| a.not_after_seq.is_some_and(|n| n > MAX_SAFE)) {
            return f("trusted_attestors", "not_after_seq must be at most 2^53 - 1");
        }
        if self.flag_threshold == 0 {
            return f("flag_threshold", "must be at least 1");
        }
        if self.flag_thresholds.values().any(|&t| t == 0) {
            return f("flag_thresholds", "values must be at least 1");
        }
        if self.seller_hidden_listing_limit == 0 {
            return f("seller_hidden_listing_limit", "must be at least 1");
        }
        let mut seen = std::collections::BTreeSet::new();
        for a in &self.trusted_attestors {
            if !seen.insert(a.author) {
                return f("trusted_attestors", "authors must be unique");
            }
            if a.kinds.is_empty() {
                return f("trusted_attestors", "kinds must not be empty");
            }
            let mut ks = std::collections::BTreeSet::new();
            for k in &a.kinds {
                if !ATTESTOR_KINDS.contains(&k.as_str()) {
                    return f("trusted_attestors", "unknown kind");
                }
                if !ks.insert(k) {
                    return f("trusted_attestors", "duplicate kind");
                }
            }
        }
        Ok(())
    }

    /// The threshold for hiding a chain of `kind`.
    pub fn threshold(&self, kind: &str) -> u64 {
        self.flag_thresholds.get(kind).copied().unwrap_or(self.flag_threshold)
    }

    /// The policy entry of `author`, if trusted for anything.
    pub fn attestor(&self, author: &AuthorId) -> Option<&TrustedAttestor> {
        self.trusted_attestors.iter().find(|a| &a.author == author)
    }

    /// True if `author` is trusted for attestation `kind`.
    pub fn trusts(&self, author: &AuthorId, kind: &str) -> bool {
        self.attestor(author).is_some_and(|a| a.kinds.iter().any(|k| k == kind))
    }

    /// The `not_after_seq` cap of `author` (`u64::MAX` when none).
    pub fn cap(&self, author: &AuthorId) -> u64 {
        self.attestor(author).and_then(|a| a.not_after_seq).unwrap_or(u64::MAX)
    }

    /// True if a record with `ts` is treated as not yet existing.
    pub fn is_future(&self, ts: u64) -> bool {
        ts > self.now.saturating_add(self.future_ts_tolerance_ms)
    }

    /// The policy as JSON.
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    /// The normalized form that is hashed (docs/views.md section 2.3).
    pub fn normalized(&self) -> Value {
        let mut v = self.to_value();
        let Some(m) = v.as_object_mut() else { return v };
        m.remove("now");
        if let Some(Value::Array(list)) = m.get_mut("trusted_attestors") {
            for a in list.iter_mut() {
                if let Some(Value::Array(k)) = a.get_mut("kinds") {
                    k.sort_by(|x, y| x.as_str().cmp(&y.as_str()));
                }
            }
            list.sort_by(|x, y| x["author"].as_str().cmp(&y["author"].as_str()));
        }
        strip_nulls(&mut v);
        v
    }
}

fn strip_nulls(v: &mut Value) {
    match v {
        Value::Object(m) => {
            m.retain(|_, x| !x.is_null());
            for x in m.values_mut() {
                strip_nulls(x);
            }
        }
        Value::Array(a) => a.iter_mut().for_each(strip_nulls),
        _ => {}
    }
}

/// `lowercase_hex(sha256(canonical_json(normalized policy)))` (ARCHITECTURE 7.1).
///
/// Never panics. For a valid policy the encoding is RFC 8785. An invalid
/// policy (for example an integer above 2^53 − 1 built in code) still gets a
/// hash, of its integers written in full.
pub fn policy_hash(policy: &Policy) -> String {
    let c = crate::canon::canonical_json(&policy.normalized());
    perch_core::ids::encode_hex(&perch_core::sha256(c.as_bytes()))
}

/// The v1 default policy (ARCHITECTURE 7.1) at `now`, trusting each given
/// attestor for all three attestation kinds.
pub fn default_policy(now: u64, attestors: &[AuthorId]) -> Policy {
    let mut list: Vec<AuthorId> = attestors.to_vec();
    list.sort();
    list.dedup();
    Policy {
        now,
        trusted_attestors: list
            .into_iter()
            .map(|author| TrustedAttestor {
                author,
                kinds: ATTESTOR_KINDS.iter().map(|s| (*s).to_owned()).collect(),
                not_after_seq: None,
            })
            .collect(),
        flag_threshold: 3,
        flag_thresholds: [("product".to_owned(), 5), ("review".to_owned(), 5)].into_iter().collect(),
        flags_require_verified: true,
        seller_hidden_listing_limit: 3,
        seller_min_distinct_flaggers: 9,
        verified_purchase_requires_verified_buyer: true,
        price_requires_domain_verified: true,
        future_ts_tolerance_ms: 600_000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_ignores_now_order_and_nulls() {
        let a = AuthorId([1; 32]);
        let b = AuthorId([2; 32]);
        let p1 = default_policy(1, &[a, b]);
        let mut p2 = default_policy(2, &[b, a]);
        p2.trusted_attestors.reverse();
        p2.trusted_attestors[0].kinds.reverse();
        assert_eq!(policy_hash(&p1), policy_hash(&p2));
        let mut p3 = p1.clone();
        p3.trusted_attestors[0].not_after_seq = Some(5);
        assert_ne!(policy_hash(&p1), policy_hash(&p3));
    }

    #[test]
    fn large_integers_rejected_and_hash_never_panics() {
        let a = AuthorId([1; 32]);
        let big = 9_007_199_254_740_992u64;
        let base = default_policy(1, &[a]);
        for path in ["now", "flag_threshold", "seller_hidden_listing_limit", "seller_min_distinct_flaggers", "future_ts_tolerance_ms"] {
            let mut v = base.to_value();
            v[path] = big.into();
            assert!(Policy::from_value(v).is_err(), "{path}");
        }
        let mut v = base.to_value();
        v["trusted_attestors"][0]["not_after_seq"] = 10_000_000_000_000_000u64.into();
        assert!(Policy::from_value(v).is_err());
        let mut v = base.to_value();
        v["flag_thresholds"]["review"] = big.into();
        assert!(Policy::from_value(v).is_err());
        let mut p = base.clone();
        p.flag_threshold = u64::MAX;
        p.trusted_attestors[0].not_after_seq = Some(u64::MAX);
        assert_eq!(policy_hash(&p).len(), 64);
        // Valid policies hash with RFC 8785 as before.
        let c = perch_core::to_canonical_json(&base.normalized()).unwrap();
        assert_eq!(policy_hash(&base), perch_core::ids::encode_hex(&perch_core::sha256(c.as_bytes())));
    }

    #[test]
    fn json_roundtrip_and_validation() {
        let p = default_policy(1_760_000_000_000, &[AuthorId([3; 32])]);
        let text = serde_json::to_string(&p).unwrap();
        assert_eq!(Policy::from_json(&text).unwrap(), p);
        let mut bad = p.to_value();
        bad["flag_threshold"] = 0.into();
        assert!(Policy::from_value(bad).is_err());
        let mut unknown = p.to_value();
        unknown["extra"] = 1.into();
        assert!(Policy::from_value(unknown).is_err());
        let mut no_cap = p.to_value();
        no_cap["trusted_attestors"][0].as_object_mut().unwrap().remove("not_after_seq");
        assert_eq!(Policy::from_value(no_cap).unwrap(), p);
    }
}
