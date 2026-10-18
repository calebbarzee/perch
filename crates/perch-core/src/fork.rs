//! Fork evidence pairs (ARCHITECTURE 5.2) and log-position helpers.
//!
//! An [`EvidencePair`] is two isolated-valid records by one author that prove
//! the author signed two different histories:
//!
//! 1. same `seq`, different IDs (fork at that seq); or
//! 2. consecutive seqs `k − 1` and `k` where the record at `k` has a `prev`
//!    different from the ID of the record at `k − 1` (fork at `k − 1`).
//!
//! The pair is stored as `(a, b)` sorted by `(seq, id)`. Pairs are totally
//! ordered by `(fork seq, a.seq, a.id, b.seq, b.id)`; stores keep the lowest.

use core::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::ids::{AuthorId, RecordId};
use crate::record::Record;

/// Why two records are not fork evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForkError {
    /// The records have different authors.
    #[error("records have different authors")]
    DifferentAuthors,
    /// The records are the same record.
    #[error("records are the same record")]
    SameRecord,
    /// Neither rule of ARCHITECTURE 5.2 applies.
    #[error("records do not prove a fork")]
    NotAFork,
}

/// Which rule of ARCHITECTURE 5.2 a pair satisfies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForkType {
    /// Same seq, different IDs.
    SameSeq,
    /// Consecutive seqs; `b.prev ≠ a.id`.
    PrevMismatch,
}

/// A verified fork evidence pair `(a, b)`, sorted by `(seq, id)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidencePair {
    a: Record,
    b: Record,
}

/// Returns the fork type if `x` and `y` (in any order) are fork evidence.
pub fn fork_type(x: &Record, y: &Record) -> Result<ForkType, ForkError> {
    if x.author() != y.author() {
        return Err(ForkError::DifferentAuthors);
    }
    if x.id() == y.id() {
        return Err(ForkError::SameRecord);
    }
    let (lo, hi) = if (x.seq(), x.id()) <= (y.seq(), y.id()) { (x, y) } else { (y, x) };
    if lo.seq() == hi.seq() {
        Ok(ForkType::SameSeq)
    } else if hi.seq() == lo.seq() + 1 && hi.prev() != Some(lo.id()) {
        Ok(ForkType::PrevMismatch)
    } else {
        Err(ForkError::NotAFork)
    }
}

impl EvidencePair {
    /// Checks that `x` and `y` are fork evidence and sorts them.
    pub fn new(x: Record, y: Record) -> Result<EvidencePair, ForkError> {
        fork_type(&x, &y)?;
        if (x.seq(), x.id()) <= (y.seq(), y.id()) {
            Ok(EvidencePair { a: x, b: y })
        } else {
            Ok(EvidencePair { a: y, b: x })
        }
    }

    /// The record with the lower `(seq, id)`.
    pub fn a(&self) -> &Record {
        &self.a
    }
    /// The record with the higher `(seq, id)`.
    pub fn b(&self) -> &Record {
        &self.b
    }
    /// Both records, `a` first.
    pub fn records(&self) -> [&Record; 2] {
        [&self.a, &self.b]
    }
    /// Both IDs, `a` first (the `evidence` hint in sync heads).
    pub fn ids(&self) -> [RecordId; 2] {
        [self.a.id(), self.b.id()]
    }
    /// The forked author.
    pub fn author(&self) -> AuthorId {
        self.a.author()
    }
    /// Which rule the pair satisfies.
    pub fn fork_type(&self) -> ForkType {
        if self.a.seq() == self.b.seq() { ForkType::SameSeq } else { ForkType::PrevMismatch }
    }
    /// The fork seq: `a.seq` for both rules (for rule 2, `a` is at `k − 1`).
    pub fn fork_seq(&self) -> u64 {
        self.a.seq()
    }
    /// The total-order key `(fork seq, a.seq, a.id, b.seq, b.id)`.
    pub fn order_key(&self) -> (u64, u64, RecordId, u64, RecordId) {
        (self.fork_seq(), self.a.seq(), self.a.id(), self.b.seq(), self.b.id())
    }
    /// Returns whichever of `self` and `other` is lower in the total order.
    pub fn lowest(self, other: EvidencePair) -> EvidencePair {
        if other < self { other } else { self }
    }
    /// The pair in its wire form: `[record a, record b]`.
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::Value::Array(vec![self.a.to_value(), self.b.to_value()])
    }
}

impl PartialOrd for EvidencePair {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for EvidencePair {
    fn cmp(&self, other: &Self) -> Ordering {
        self.order_key().cmp(&other.order_key())
    }
}

impl Serialize for EvidencePair {
    /// Serializes as `[record a, record b]`.
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut seq = s.serialize_seq(Some(2))?;
        seq.serialize_element(&self.a)?;
        seq.serialize_element(&self.b)?;
        seq.end()
    }
}

impl<'de> Deserialize<'de> for EvidencePair {
    /// Deserializes `[record, record]` in either order and verifies the pair.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let (x, y) = <(Record, Record)>::deserialize(d)?;
        EvidencePair::new(x, y).map_err(serde::de::Error::custom)
    }
}

/// The `seq` and `prev` an author's next record must use, given the
/// author's current head `(seq, id)` (or `None` for an empty log).
pub fn next_position(head: Option<(u64, RecordId)>) -> (u64, Option<RecordId>) {
    match head {
        None => (1, None),
        Some((seq, id)) => (seq + 1, Some(id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Keypair, Unsigned, sign_record};
    use serde_json::{Map, json};

    fn rec(kp: &Keypair, seq: u64, prev: Option<RecordId>, n: u64) -> Record {
        let mut body = Map::new();
        body.insert("n".into(), json!(n));
        sign_record(kp, Unsigned { seq, prev, ts: 10, kind: "x".into(), replaces: None, body }).unwrap()
    }

    #[test]
    fn pair_rules_and_order() {
        let kp = Keypair::from_seed([11; 32]);
        let r1 = rec(&kp, 1, None, 0);
        let r1b = rec(&kp, 1, None, 1);
        let r2 = rec(&kp, 2, Some(r1.id()), 0);
        let r2_other = rec(&kp, 2, Some(r1b.id()), 0);
        let r3 = rec(&kp, 3, Some(r2.id()), 0);

        let p = EvidencePair::new(r1b.clone(), r1.clone()).unwrap();
        assert_eq!(p.fork_type(), ForkType::SameSeq);
        assert_eq!(p.fork_seq(), 1);
        assert!(p.a().id() < p.b().id());

        let q = EvidencePair::new(r2_other.clone(), r1.clone()).unwrap();
        assert_eq!(q.fork_type(), ForkType::PrevMismatch);
        assert_eq!(q.fork_seq(), 1);
        assert_eq!(q.a().id(), r1.id());

        assert_eq!(fork_type(&r1, &r2), Err(ForkError::NotAFork));
        assert_eq!(fork_type(&r1, &r3), Err(ForkError::NotAFork));
        assert_eq!(fork_type(&r1, &r1), Err(ForkError::SameRecord));
        let other = rec(&Keypair::from_seed([12; 32]), 1, None, 0);
        assert_eq!(fork_type(&r1, &other), Err(ForkError::DifferentAuthors));

        // Order: both at fork seq 1; p has a.seq = 1, b.seq = 1; q has b.seq = 2.
        let pk = p.order_key();
        let qk = q.order_key();
        assert_eq!(p.clone().lowest(q.clone()).order_key(), pk.min(qk));

        // Serde round trip, either order.
        let v = serde_json::to_value(&q).unwrap();
        let back: EvidencePair = serde_json::from_value(v).unwrap();
        assert_eq!(back, q);
        let swapped = serde_json::json!([r1.to_value(), r2_other.to_value()]);
        assert_eq!(serde_json::from_value::<EvidencePair>(swapped).unwrap(), q);
        assert!(serde_json::from_value::<EvidencePair>(json!([r1.to_value(), r2.to_value()])).is_err());
        // Pairs verify without bodies.
        assert!(EvidencePair::new(r1.without_body(), r1b.without_body()).is_ok());
    }

    #[test]
    fn next_position_helper() {
        assert_eq!(next_position(None), (1, None));
        let id = RecordId([1; 32]);
        assert_eq!(next_position(Some((4, id))), (5, Some(id)));
    }
}
