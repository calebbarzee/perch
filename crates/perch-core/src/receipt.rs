//! Detached purchase receipts (ARCHITECTURE 4.7).
//!
//! A receipt is signed by the seller over
//! `"perch/receipt/v1\n" || canonical_json({ listing, buyer, purchased_at, nonce })`.
//! It is not a log record. Core checks only the signature; whether the
//! listing exists and who authored it is decided by views.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ids::{AuthorId, Hex32, RecordId, Sig};
use crate::keys::{self, Keypair, SignatureError};
use crate::limits::{MAX_SAFE_INTEGER, RECEIPT_DOMAIN};

/// A signed receipt. Serializes as
/// `{ "listing", "buyer", "purchased_at", "nonce", "sig" }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    /// Root ID of the seller's listing.
    pub listing: RecordId,
    /// The buyer's author ID.
    pub buyer: AuthorId,
    /// Purchase time, Unix milliseconds.
    pub purchased_at: u64,
    /// 32 random bytes chosen by the seller.
    pub nonce: Hex32,
    /// Seller's signature.
    pub sig: Sig,
}

/// The signed content of a receipt.
fn receipt_canonical(listing: &RecordId, buyer: &AuthorId, purchased_at: u64, nonce: &Hex32) -> String {
    // Keys in UTF-16 order: buyer, listing, nonce, purchased_at.
    format!(
        "{{\"buyer\":\"{}\",\"listing\":\"{}\",\"nonce\":\"{}\",\"purchased_at\":{}}}",
        buyer, listing, nonce, purchased_at
    )
}

impl Receipt {
    /// The canonical JSON that the seller signs (without the domain prefix).
    pub fn signed_canonical(&self) -> String {
        receipt_canonical(&self.listing, &self.buyer, self.purchased_at, &self.nonce)
    }

    /// Strictly verifies the signature under `seller` (the author of the
    /// listing chain).
    pub fn verify(&self, seller: &AuthorId) -> Result<(), SignatureError> {
        verify_receipt(self, seller)
    }

    /// The receipt as a JSON object (for a review body's `receipt` field).
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("receipt serializes")
    }
}

/// Signs a receipt as the seller. `nonce` must be 32 random bytes; the
/// caller supplies it because wasm has no randomness in this crate.
pub fn sign_receipt(
    seller: &Keypair,
    listing: RecordId,
    buyer: AuthorId,
    purchased_at: u64,
    nonce: [u8; 32],
) -> Result<Receipt, ReceiptError> {
    if purchased_at > MAX_SAFE_INTEGER {
        return Err(ReceiptError::OutOfRange);
    }
    let nonce = Hex32(nonce);
    let msg = receipt_canonical(&listing, &buyer, purchased_at, &nonce);
    let sig = seller.sign_with_domain(RECEIPT_DOMAIN, msg.as_bytes());
    Ok(Receipt { listing, buyer, purchased_at, nonce, sig })
}

/// Strictly verifies `receipt.sig` under `seller`.
pub fn verify_receipt(receipt: &Receipt, seller: &AuthorId) -> Result<(), SignatureError> {
    keys::verify_with_domain(seller, RECEIPT_DOMAIN, receipt.signed_canonical().as_bytes(), &receipt.sig)
}

/// Why a receipt is malformed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum ReceiptError {
    /// The receipt is not a JSON object.
    #[error("receipt is not a JSON object")]
    NotObject,
    /// A field other than listing, buyer, purchased_at, nonce, sig.
    #[error("unknown receipt field {field:?}")]
    UnknownField {
        /// Field name.
        field: String,
    },
    /// A required field is missing.
    #[error("missing receipt field {field}")]
    MissingField {
        /// Field name.
        field: &'static str,
    },
    /// A field has the wrong type or format.
    #[error("receipt field {field}: {problem}")]
    InvalidField {
        /// Field name.
        field: &'static str,
        /// What is wrong.
        problem: &'static str,
    },
    /// `purchased_at` is outside 0 to 2^53 − 1.
    #[error("purchased_at is outside 0 to 2^53 - 1")]
    OutOfRange,
}

/// Parses a receipt object strictly. Unknown fields are rejected: the
/// receipt is a fixed signed structure, and a new format would use a new
/// signature domain (ARCHITECTURE 4.3).
pub fn parse_receipt(v: &Value) -> Result<Receipt, ReceiptError> {
    let Value::Object(m) = v else {
        return Err(ReceiptError::NotObject);
    };
    parse_receipt_map(m)
}

fn s<'a>(m: &'a Map<String, Value>, field: &'static str) -> Result<&'a str, ReceiptError> {
    match m.get(field) {
        None => Err(ReceiptError::MissingField { field }),
        Some(Value::String(s)) => Ok(s),
        Some(_) => Err(ReceiptError::InvalidField { field, problem: "must be a string" }),
    }
}

fn parse_receipt_map(m: &Map<String, Value>) -> Result<Receipt, ReceiptError> {
    for k in m.keys() {
        if !["listing", "buyer", "purchased_at", "nonce", "sig"].contains(&k.as_str()) {
            return Err(ReceiptError::UnknownField { field: k.clone() });
        }
    }
    let hex = |field: &'static str| ReceiptError::InvalidField { field, problem: "must be lowercase hex of the right length" };
    let listing = RecordId::from_hex(s(m, "listing")?).map_err(|_| hex("listing"))?;
    let buyer = AuthorId::from_hex(s(m, "buyer")?).map_err(|_| hex("buyer"))?;
    let nonce = Hex32::from_hex(s(m, "nonce")?).map_err(|_| hex("nonce"))?;
    let sig = Sig::from_hex(s(m, "sig")?).map_err(|_| hex("sig"))?;
    let purchased_at = match m.get("purchased_at") {
        None => return Err(ReceiptError::MissingField { field: "purchased_at" }),
        Some(Value::Number(n)) => match n.as_u64() {
            Some(u) if u <= MAX_SAFE_INTEGER => u,
            _ => return Err(ReceiptError::OutOfRange),
        },
        Some(_) => return Err(ReceiptError::InvalidField { field: "purchased_at", problem: "must be an integer" }),
    };
    Ok(Receipt { listing, buyer, purchased_at, nonce, sig })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sign_and_verify() {
        let seller = Keypair::from_seed([3; 32]);
        let buyer = Keypair::from_seed([4; 32]).author();
        let r = sign_receipt(&seller, RecordId([5; 32]), buyer, 1_700_000_000_000, [6; 32]).unwrap();
        assert_eq!(verify_receipt(&r, &seller.author()), Ok(()));
        assert!(r.verify(&buyer).is_err());
        let mut bad = r.clone();
        bad.purchased_at += 1;
        assert_eq!(bad.verify(&seller.author()), Err(SignatureError::Invalid));
        let v = r.to_value();
        assert_eq!(parse_receipt(&v).unwrap(), r);
        let text = crate::canonical::to_canonical_json(&json!({
            "listing": r.listing, "buyer": r.buyer, "purchased_at": r.purchased_at, "nonce": r.nonce
        }))
        .unwrap();
        assert_eq!(text, r.signed_canonical());
        // A record signature over the same bytes is not a receipt signature.
        let s2 = seller.sign_with_domain(crate::limits::RECORD_DOMAIN, text.as_bytes());
        assert!(verify_receipt(&Receipt { sig: s2, ..r.clone() }, &seller.author()).is_err());
    }

    #[test]
    fn parse_errors() {
        let seller = Keypair::from_seed([3; 32]);
        let r = sign_receipt(&seller, RecordId([5; 32]), seller.author(), 1, [6; 32]).unwrap();
        let mut v = r.to_value();
        v["extra"] = json!(1);
        assert!(matches!(parse_receipt(&v), Err(ReceiptError::UnknownField { .. })));
        let mut v = r.to_value();
        v.as_object_mut().unwrap().remove("nonce");
        assert_eq!(parse_receipt(&v), Err(ReceiptError::MissingField { field: "nonce" }));
        let mut v = r.to_value();
        v["sig"] = json!(r.sig.to_hex().to_uppercase());
        assert!(matches!(parse_receipt(&v), Err(ReceiptError::InvalidField { field: "sig", .. })));
        let mut v = r.to_value();
        v["purchased_at"] = json!(-1);
        assert_eq!(parse_receipt(&v), Err(ReceiptError::OutOfRange));
        assert_eq!(parse_receipt(&json!([])), Err(ReceiptError::NotObject));
        assert!(sign_receipt(&seller, RecordId([5; 32]), seller.author(), 1 << 53, [6; 32]).is_err());
    }
}
