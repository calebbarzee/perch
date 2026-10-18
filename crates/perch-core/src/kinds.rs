//! Body schemas for the v1 kinds (ARCHITECTURE 4.5) and kind metadata.
//!
//! These are **semantic** rules: a record whose body fails them stays
//! stored and relayed; views ignore it and can report the [`SchemaError`].
//!
//! Rules applied to every kind:
//!
//! - Text lengths count Unicode scalar values (Rust `char`s).
//! - Unknown body fields are allowed and ignored, at every nesting level,
//!   except inside a `receipt` (a fixed signed structure, see
//!   [`crate::receipt`]).
//! - Optional fields are absent or of the stated type; `null` is invalid.
//! - Integers are JSON integers in 0 to 2^53 − 1 unless stated otherwise.
//! - Country codes are two ASCII uppercase letters (ISO 3166-1 alpha-2
//!   syntax; assignment is not checked).
//! - Currency codes must be active ISO 4217 codes with defined minor units
//!   ([`crate::currency`]).

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::currency;
use crate::ids::{AuthorId, Hex32, RecordId};
use crate::limits::MAX_SAFE_INTEGER;
use crate::product_key::{self, ProductKey, ProductKeyError};
use crate::receipt::{self, Receipt, ReceiptError};
use crate::record::Record;
use crate::weburl::{self, UrlError};

// ---------------------------------------------------------------------------
// Kind metadata
// ---------------------------------------------------------------------------

/// Kind names of v1.
pub mod names {
    /// `profile`
    pub const PROFILE: &str = "profile";
    /// `attest.identity`
    pub const ATTEST_IDENTITY: &str = "attest.identity";
    /// `attest.domain`
    pub const ATTEST_DOMAIN: &str = "attest.domain";
    /// `attest.revoke`
    pub const ATTEST_REVOKE: &str = "attest.revoke";
    /// `product`
    pub const PRODUCT: &str = "product";
    /// `listing`
    pub const LISTING: &str = "listing";
    /// `observation`
    pub const OBSERVATION: &str = "observation";
    /// `review`
    pub const REVIEW: &str = "review";
    /// `flag`
    pub const FLAG: &str = "flag";
    /// `appeal`
    pub const APPEAL: &str = "appeal";
    /// `retract`
    pub const RETRACT: &str = "retract";
    /// `retire`
    pub const RETIRE: &str = "retire";
    /// `redact`
    pub const REDACT: &str = "redact";
}

/// Static facts about a v1 kind (ARCHITECTURE 4.5 "Replaceable" column,
/// control kinds, and `retract` targets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct KindInfo {
    /// Kind name.
    pub name: &'static str,
    /// Records of this kind may carry `replaces` and may be replaced.
    pub replaceable: bool,
    /// Body fields that must not change across versions of a chain.
    /// See also [`KindInfo::fixed_if_present`].
    pub fixed: &'static [&'static str],
    /// `(field, condition)`: `field` is fixed when the body has `condition`.
    /// Only `product.brand` (fixed when `mpn` is set) in v1.
    pub fixed_if_present: &'static [(&'static str, &'static str)],
    /// Control kind: a node must not remove its body (ARCHITECTURE 4.5, 5.3).
    pub control: bool,
    /// May be the target of a `retract`.
    pub retractable: bool,
}

/// All v1 kinds, sorted by name.
pub static KINDS: &[KindInfo] = &[
    KindInfo { name: names::APPEAL, replaceable: true, fixed: &["target"], fixed_if_present: &[], control: false, retractable: true },
    KindInfo { name: names::ATTEST_DOMAIN, replaceable: true, fixed: &["subject", "domain"], fixed_if_present: &[], control: true, retractable: true },
    KindInfo { name: names::ATTEST_IDENTITY, replaceable: true, fixed: &["subject", "person"], fixed_if_present: &[], control: true, retractable: true },
    KindInfo { name: names::ATTEST_REVOKE, replaceable: false, fixed: &[], fixed_if_present: &[], control: true, retractable: false },
    KindInfo { name: names::FLAG, replaceable: true, fixed: &["target"], fixed_if_present: &[], control: false, retractable: true },
    KindInfo { name: names::LISTING, replaceable: true, fixed: &["product"], fixed_if_present: &[], control: false, retractable: true },
    KindInfo { name: names::OBSERVATION, replaceable: false, fixed: &[], fixed_if_present: &[], control: false, retractable: true },
    KindInfo { name: names::PRODUCT, replaceable: true, fixed: &["gtin", "mpn"], fixed_if_present: &[("brand", "mpn")], control: false, retractable: true },
    KindInfo { name: names::PROFILE, replaceable: true, fixed: &[], fixed_if_present: &[], control: false, retractable: true },
    KindInfo { name: names::REDACT, replaceable: false, fixed: &[], fixed_if_present: &[], control: false, retractable: false },
    KindInfo { name: names::RETIRE, replaceable: false, fixed: &[], fixed_if_present: &[], control: true, retractable: false },
    KindInfo { name: names::RETRACT, replaceable: false, fixed: &[], fixed_if_present: &[], control: true, retractable: false },
    KindInfo { name: names::REVIEW, replaceable: true, fixed: &["product"], fixed_if_present: &[], control: false, retractable: true },
];

/// Facts about a v1 kind, or `None` for an unknown kind.
pub fn kind_info(kind: &str) -> Option<&'static KindInfo> {
    KINDS.binary_search_by(|k| k.name.cmp(kind)).ok().map(|i| &KINDS[i])
}

/// True for a v1 kind.
pub fn is_known_kind(kind: &str) -> bool {
    kind_info(kind).is_some()
}

/// True for control kinds: `attest.identity`, `attest.domain`,
/// `attest.revoke`, `retract`, `retire`. Their bodies must not be removed.
pub fn is_control_kind(kind: &str) -> bool {
    kind_info(kind).is_some_and(|k| k.control)
}

/// True if records of `kind` may carry `replaces` and be replaced.
/// Unknown kinds return `false`.
pub fn is_replaceable(kind: &str) -> bool {
    kind_info(kind).is_some_and(|k| k.replaceable)
}

/// True if records of `kind` may be the target of a `retract`.
/// Unknown kinds return `true` (views ignore unknown kinds anyway).
pub fn is_retractable(kind: &str) -> bool {
    kind_info(kind).is_none_or(|k| k.retractable)
}

/// The fields of `kind` that are fixed across versions, given the body of
/// the earlier version. For `product`, `brand` is included when `old` has
/// `mpn`.
pub fn fixed_fields(kind: &str, old: &Map<String, Value>) -> Vec<&'static str> {
    let Some(info) = kind_info(kind) else {
        return Vec::new();
    };
    let mut v: Vec<&'static str> = info.fixed.to_vec();
    for (field, cond) in info.fixed_if_present {
        if old.contains_key(*cond) {
            v.push(field);
        }
    }
    v
}

/// Checks that `new` keeps every fixed field of `old` (ARCHITECTURE 4.6).
/// Fields are compared as raw JSON values; presence must also match.
/// Returns the first changed field (in [`fixed_fields`] order) as an error.
pub fn check_fixed_fields(kind: &str, old: &Map<String, Value>, new: &Map<String, Value>) -> Result<(), SchemaError> {
    for f in fixed_fields(kind, old) {
        if old.get(f) != new.get(f) {
            return Err(SchemaError::new(f, Problem::FixedFieldChanged));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// What is wrong with a body field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "problem", rename_all = "snake_case")]
pub enum Problem {
    /// The record has no body (removed or not yet received).
    BodyMissing,
    /// A required field is absent.
    Missing,
    /// Wrong JSON type.
    WrongType {
        /// Expected JSON type.
        expected: &'static str,
    },
    /// Text shorter than `min` scalar values.
    TooShort {
        /// Minimum length.
        min: usize,
    },
    /// Text longer than `max` scalar values.
    TooLong {
        /// Maximum length.
        max: usize,
    },
    /// List or object with more than `max` entries.
    TooMany {
        /// Maximum number of entries.
        max: usize,
    },
    /// Integer outside `min..=max`.
    OutOfRange {
        /// Minimum.
        min: u64,
        /// Maximum.
        max: u64,
    },
    /// Not one of the allowed values.
    NotAllowed {
        /// The allowed values.
        allowed: &'static [&'static str],
    },
    /// Not lowercase hex of the right length.
    BadHex,
    /// Not two ASCII uppercase letters.
    BadCountry,
    /// Not an active ISO 4217 code with defined minor units.
    BadCurrency,
    /// Invalid GTIN.
    BadGtin {
        /// Detail.
        detail: ProductKeyError,
    },
    /// Invalid product key.
    BadProductKey {
        /// Detail.
        detail: ProductKeyError,
    },
    /// Invalid URL or hostname.
    BadUrl {
        /// Detail.
        detail: UrlError,
    },
    /// Invalid category path.
    BadCategory,
    /// Malformed receipt.
    BadReceipt {
        /// Detail.
        detail: ReceiptError,
    },
    /// `legal_name` given for `level` other than `business`.
    OnlyForBusiness,
    /// `observation.observed_at` is after the record's `ts`.
    AfterRecordTs,
    /// The record carries `replaces` but its kind is not replaceable.
    NotReplaceable,
    /// A fixed field differs from the replaced version.
    FixedFieldChanged,
}

/// A semantic body error: the field path (for example `price.currency` or
/// `images[2].url`) and the problem. Serializes as
/// `{ "field": "...", "problem": "<snake_case>", ...details }`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[error("{field}: {problem:?}")]
pub struct SchemaError {
    /// Field path within the body; `"body"` or `"replaces"` for record-level problems.
    pub field: String,
    /// What is wrong.
    #[serde(flatten)]
    pub problem: Problem,
}

impl SchemaError {
    fn new(field: impl Into<String>, problem: Problem) -> Self {
        SchemaError { field: field.into(), problem }
    }
}

type R<T> = Result<T, SchemaError>;

// ---------------------------------------------------------------------------
// Typed bodies
// ---------------------------------------------------------------------------

/// `profile.role`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// `seller`
    Seller,
    /// `buyer`
    Buyer,
    /// `tester`
    Tester,
    /// `crawler`
    Crawler,
    /// `attestor`
    Attestor,
    /// `node`
    Node,
    /// `other`
    Other,
}

/// `attest.identity.level`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// `person`
    Person,
    /// `business`
    Business,
}

/// `listing.availability` and `observation.availability`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    /// `in_stock`
    InStock,
    /// `out_of_stock`
    OutOfStock,
    /// `preorder`
    Preorder,
    /// `discontinued`
    Discontinued,
}

/// `listing.condition`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    /// `new`
    New,
    /// `used`
    Used,
    /// `refurbished`
    Refurbished,
}

/// `flag.reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagReason {
    /// `fraud`
    Fraud,
    /// `counterfeit`
    Counterfeit,
    /// `misleading`
    Misleading,
    /// `spam`
    Spam,
    /// `illegal`
    Illegal,
    /// `personal_data`
    PersonalData,
    /// `other`
    Other,
}

/// `redact.reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RedactReason {
    /// `illegal`
    Illegal,
    /// `personal_data`
    PersonalData,
    /// `court_order`
    CourtOrder,
    /// `other`
    Other,
}

macro_rules! str_enum {
    ($t:ty, $allowed:ident, [$($s:literal => $v:expr),+ $(,)?]) => {
        /// Allowed string values.
        pub const $allowed: &[&str] = &[$($s),+];
        impl $t {
            /// Parses the JSON string form.
            pub fn parse(s: &str) -> Option<Self> {
                match s { $($s => Some($v),)+ _ => None }
            }
            /// The JSON string form.
            pub fn as_str(&self) -> &'static str {
                $(if *self == $v { return $s; })+
                unreachable!()
            }
        }
    };
}

str_enum!(Role, ROLES, ["seller" => Role::Seller, "buyer" => Role::Buyer, "tester" => Role::Tester, "crawler" => Role::Crawler, "attestor" => Role::Attestor, "node" => Role::Node, "other" => Role::Other]);
str_enum!(Level, LEVELS, ["person" => Level::Person, "business" => Level::Business]);
str_enum!(Availability, AVAILABILITIES, ["in_stock" => Availability::InStock, "out_of_stock" => Availability::OutOfStock, "preorder" => Availability::Preorder, "discontinued" => Availability::Discontinued]);
str_enum!(Condition, CONDITIONS, ["new" => Condition::New, "used" => Condition::Used, "refurbished" => Condition::Refurbished]);
str_enum!(FlagReason, FLAG_REASONS, ["fraud" => FlagReason::Fraud, "counterfeit" => FlagReason::Counterfeit, "misleading" => FlagReason::Misleading, "spam" => FlagReason::Spam, "illegal" => FlagReason::Illegal, "personal_data" => FlagReason::PersonalData, "other" => FlagReason::Other]);
str_enum!(RedactReason, REDACT_REASONS, ["illegal" => RedactReason::Illegal, "personal_data" => RedactReason::PersonalData, "court_order" => RedactReason::CourtOrder, "other" => RedactReason::Other]);

/// `{ "amount", "currency" }`: an amount in the currency's minor unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Price {
    /// Amount in minor units (for example cents), ≥ 0.
    pub amount: u64,
    /// Active ISO 4217 code with defined minor units.
    pub currency: String,
}

/// `profile` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Profile {
    /// 1–100 characters.
    pub name: String,
    /// Role.
    pub role: Role,
    /// URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// ≤ 2000 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
}

/// `attest.identity` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttestIdentity {
    /// The attested author.
    pub subject: AuthorId,
    /// Stable pseudonym of the human or business.
    pub person: Hex32,
    /// `person` or `business`.
    pub level: Level,
    /// 1–200 characters; only when `level` is `business`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legal_name: Option<String>,
    /// ISO 3166-1 alpha-2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    /// Expiry, Unix ms.
    pub expires: u64,
}

/// `attest.domain` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttestDomain {
    /// The attested author.
    pub subject: AuthorId,
    /// Lowercase ASCII hostname, ≤ 253 bytes.
    pub domain: String,
    /// Expiry, Unix ms.
    pub expires: u64,
}

/// `attest.revoke` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttestRevoke {
    /// The author whose key is compromised.
    pub subject: AuthorId,
    /// Records of `subject` with a higher seq are ignored.
    pub valid_through_seq: u64,
}

/// One entry of `product.images`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Image {
    /// URL.
    pub url: String,
    /// SHA-256 of the image file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<Hex32>,
}

/// `product` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Product {
    /// 1–300 characters.
    pub name: String,
    /// 1–100 characters.
    pub brand: String,
    /// GTIN as written in the body (8, 12, 13, or 14 digits).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gtin: Option<String>,
    /// 1–100 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mpn: Option<String>,
    /// ≤ 200 characters, lowercase, `/`-separated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// ≤ 64 entries; keys 1–64, values ≤ 500 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<BTreeMap<String, String>>,
    /// ≤ 16 images.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<Image>>,
    /// ≤ 5000 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl Product {
    /// The product key per ARCHITECTURE 4.8, given the root ID of this
    /// product's chain.
    pub fn key(&self, root: RecordId) -> ProductKey {
        // Fields were validated by parse_body, so derivation cannot fail.
        ProductKey::derive(self.gtin.as_deref(), &self.brand, self.mpn.as_deref(), root)
            .unwrap_or_else(|_| ProductKey::from_root(root))
    }
}

/// `listing.location`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Location {
    /// ISO 3166-1 alpha-2.
    pub country: String,
    /// ≤ 100 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

/// `listing` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listing {
    /// Product key.
    pub product: ProductKey,
    /// URL.
    pub url: String,
    /// Price.
    pub price: Price,
    /// Availability.
    pub availability: Availability,
    /// Condition.
    pub condition: Condition,
    /// ≤ 300 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// ≤ 250 ISO 3166-1 alpha-2 codes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ships_to: Option<Vec<String>>,
    /// Seller location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<Location>,
}

/// `observation` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Observation {
    /// Product key.
    pub product: ProductKey,
    /// URL.
    pub url: String,
    /// Observed price.
    pub price: Price,
    /// Observation time, Unix ms; ≤ the record's `ts`.
    pub observed_at: u64,
    /// The listing observed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing: Option<RecordId>,
    /// Observed availability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability: Option<Availability>,
}

/// `review` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Review {
    /// Product key.
    pub product: ProductKey,
    /// 1–5.
    pub rating: u8,
    /// 1–10000 characters.
    pub text: String,
    /// ≤ 200 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The listing bought from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing: Option<RecordId>,
    /// Detached receipt (signature not checked here).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<Receipt>,
}

/// `flag` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Flag {
    /// Flagged record.
    pub target: RecordId,
    /// Reason.
    pub reason: FlagReason,
    /// ≤ 2000 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// `appeal` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Appeal {
    /// Appealed record.
    pub target: RecordId,
    /// 1–2000 characters.
    pub note: String,
}

/// `retract` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Retract {
    /// Withdrawn record.
    pub target: RecordId,
}

/// `retire` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Retire {
    /// Successor key (informational only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub successor: Option<AuthorId>,
}

/// `redact` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Redact {
    /// Record whose body was removed.
    pub target: RecordId,
    /// Reason.
    pub reason: RedactReason,
    /// ≤ 2000 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A parsed body. Serializes as the body object itself (untagged).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
#[allow(missing_docs)]
pub enum Body {
    Profile(Profile),
    AttestIdentity(AttestIdentity),
    AttestDomain(AttestDomain),
    AttestRevoke(AttestRevoke),
    Product(Product),
    Listing(Listing),
    Observation(Observation),
    Review(Review),
    Flag(Flag),
    Appeal(Appeal),
    Retract(Retract),
    Retire(Retire),
    Redact(Redact),
    /// A kind this software does not know. Holds the raw body.
    Unknown(Map<String, Value>),
}

impl Body {
    /// The kind name, or `None` for [`Body::Unknown`].
    pub fn kind(&self) -> Option<&'static str> {
        Some(match self {
            Body::Profile(_) => names::PROFILE,
            Body::AttestIdentity(_) => names::ATTEST_IDENTITY,
            Body::AttestDomain(_) => names::ATTEST_DOMAIN,
            Body::AttestRevoke(_) => names::ATTEST_REVOKE,
            Body::Product(_) => names::PRODUCT,
            Body::Listing(_) => names::LISTING,
            Body::Observation(_) => names::OBSERVATION,
            Body::Review(_) => names::REVIEW,
            Body::Flag(_) => names::FLAG,
            Body::Appeal(_) => names::APPEAL,
            Body::Retract(_) => names::RETRACT,
            Body::Retire(_) => names::RETIRE,
            Body::Redact(_) => names::REDACT,
            Body::Unknown(_) => return None,
        })
    }

    /// `product` field of a listing, observation, or review.
    pub fn product(&self) -> Option<&ProductKey> {
        match self {
            Body::Listing(b) => Some(&b.product),
            Body::Observation(b) => Some(&b.product),
            Body::Review(b) => Some(&b.product),
            _ => None,
        }
    }

    /// `target` field of a flag, appeal, retract, or redact.
    pub fn target(&self) -> Option<RecordId> {
        match self {
            Body::Flag(b) => Some(b.target),
            Body::Appeal(b) => Some(b.target),
            Body::Retract(b) => Some(b.target),
            Body::Redact(b) => Some(b.target),
            _ => None,
        }
    }

    /// `subject` field of an attestation or revocation.
    pub fn subject(&self) -> Option<AuthorId> {
        match self {
            Body::AttestIdentity(b) => Some(b.subject),
            Body::AttestDomain(b) => Some(b.subject),
            Body::AttestRevoke(b) => Some(b.subject),
            _ => None,
        }
    }

    /// The body as a JSON object (optional fields omitted when absent).
    pub fn to_map(&self) -> Map<String, Value> {
        to_body_map(self)
    }
}

/// Serializes any typed body into a JSON object, for [`crate::Unsigned::body`].
///
/// # Panics
/// If `body` does not serialize to a JSON object.
pub fn to_body_map<T: Serialize + ?Sized>(body: &T) -> Map<String, Value> {
    match serde_json::to_value(body) {
        Ok(Value::Object(m)) => m,
        _ => panic!("body does not serialize to a JSON object"),
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

struct Obj<'a> {
    m: &'a Map<String, Value>,
    path: String,
}

impl<'a> Obj<'a> {
    fn root(m: &'a Map<String, Value>) -> Self {
        Obj { m, path: String::new() }
    }

    fn p(&self, name: &str) -> String {
        if self.path.is_empty() { name.to_owned() } else { format!("{}.{name}", self.path) }
    }

    fn get(&self, name: &str) -> R<Option<&'a Value>> {
        match self.m.get(name) {
            None => Ok(None),
            Some(Value::Null) => Err(SchemaError::new(self.p(name), Problem::WrongType { expected: "non-null value" })),
            Some(v) => Ok(Some(v)),
        }
    }

    fn need(&self, name: &str) -> R<&'a Value> {
        self.get(name)?.ok_or_else(|| SchemaError::new(self.p(name), Problem::Missing))
    }

    fn opt_str(&self, name: &str, min: usize, max: usize) -> R<Option<String>> {
        match self.get(name)? {
            None => Ok(None),
            Some(v) => text(v, &self.p(name), min, max).map(Some),
        }
    }

    fn str(&self, name: &str, min: usize, max: usize) -> R<String> {
        text(self.need(name)?, &self.p(name), min, max)
    }

    fn raw_str(&self, name: &str) -> R<&'a str> {
        as_str(self.need(name)?, &self.p(name))
    }

    fn opt_raw_str(&self, name: &str) -> R<Option<&'a str>> {
        match self.get(name)? {
            None => Ok(None),
            Some(v) => as_str(v, &self.p(name)).map(Some),
        }
    }

    fn uint(&self, name: &str, min: u64, max: u64) -> R<u64> {
        uint(self.need(name)?, &self.p(name), min, max)
    }

    fn time(&self, name: &str) -> R<u64> {
        self.uint(name, 0, MAX_SAFE_INTEGER)
    }

    fn en<T>(&self, name: &str, allowed: &'static [&'static str], f: fn(&str) -> Option<T>) -> R<T> {
        let s = self.raw_str(name)?;
        f(s).ok_or_else(|| SchemaError::new(self.p(name), Problem::NotAllowed { allowed }))
    }

    fn opt_en<T>(&self, name: &str, allowed: &'static [&'static str], f: fn(&str) -> Option<T>) -> R<Option<T>> {
        match self.opt_raw_str(name)? {
            None => Ok(None),
            Some(s) => f(s).map(Some).ok_or_else(|| SchemaError::new(self.p(name), Problem::NotAllowed { allowed })),
        }
    }

    fn hex32(&self, name: &str) -> R<[u8; 32]> {
        let s = self.raw_str(name)?;
        crate::ids::decode_hex::<32>(s).map_err(|_| SchemaError::new(self.p(name), Problem::BadHex))
    }

    fn opt_hex32(&self, name: &str) -> R<Option<[u8; 32]>> {
        match self.opt_raw_str(name)? {
            None => Ok(None),
            Some(s) => crate::ids::decode_hex::<32>(s).map(Some).map_err(|_| SchemaError::new(self.p(name), Problem::BadHex)),
        }
    }

    fn author(&self, name: &str) -> R<AuthorId> {
        self.hex32(name).map(AuthorId)
    }
    fn id(&self, name: &str) -> R<RecordId> {
        self.hex32(name).map(RecordId)
    }
    fn opt_id(&self, name: &str) -> R<Option<RecordId>> {
        self.opt_hex32(name).map(|o| o.map(RecordId))
    }

    fn url(&self, name: &str) -> R<String> {
        let s = self.raw_str(name)?;
        check_url(s, &self.p(name))?;
        Ok(s.to_owned())
    }

    fn opt_url(&self, name: &str) -> R<Option<String>> {
        match self.opt_raw_str(name)? {
            None => Ok(None),
            Some(s) => {
                check_url(s, &self.p(name))?;
                Ok(Some(s.to_owned()))
            }
        }
    }

    fn country(&self, name: &str) -> R<String> {
        let s = self.raw_str(name)?;
        check_country(s, &self.p(name))?;
        Ok(s.to_owned())
    }

    fn opt_country(&self, name: &str) -> R<Option<String>> {
        match self.opt_raw_str(name)? {
            None => Ok(None),
            Some(s) => {
                check_country(s, &self.p(name))?;
                Ok(Some(s.to_owned()))
            }
        }
    }

    fn product_key(&self, name: &str) -> R<ProductKey> {
        let s = self.raw_str(name)?;
        ProductKey::parse(s).map_err(|detail| SchemaError::new(self.p(name), Problem::BadProductKey { detail }))
    }

    fn obj(&self, name: &str) -> R<Obj<'a>> {
        let p = self.p(name);
        match self.need(name)? {
            Value::Object(m) => Ok(Obj { m, path: p }),
            _ => Err(SchemaError::new(p, Problem::WrongType { expected: "object" })),
        }
    }

    fn opt_obj(&self, name: &str) -> R<Option<Obj<'a>>> {
        if self.get(name)?.is_none() { Ok(None) } else { self.obj(name).map(Some) }
    }

    fn opt_arr(&self, name: &str, max: usize) -> R<Option<&'a Vec<Value>>> {
        match self.get(name)? {
            None => Ok(None),
            Some(Value::Array(a)) => {
                if a.len() > max {
                    Err(SchemaError::new(self.p(name), Problem::TooMany { max }))
                } else {
                    Ok(Some(a))
                }
            }
            Some(_) => Err(SchemaError::new(self.p(name), Problem::WrongType { expected: "array" })),
        }
    }

    fn price(&self, name: &str) -> R<Price> {
        let o = self.obj(name)?;
        let amount = o.uint("amount", 0, MAX_SAFE_INTEGER)?;
        let currency = o.raw_str("currency")?;
        if !currency::is_price_currency(currency) {
            return Err(SchemaError::new(o.p("currency"), Problem::BadCurrency));
        }
        Ok(Price { amount, currency: currency.to_owned() })
    }
}

fn as_str<'a>(v: &'a Value, path: &str) -> R<&'a str> {
    match v {
        Value::String(s) => Ok(s),
        _ => Err(SchemaError::new(path, Problem::WrongType { expected: "string" })),
    }
}

fn text(v: &Value, path: &str, min: usize, max: usize) -> R<String> {
    let s = as_str(v, path)?;
    let n = s.chars().count();
    if n < min {
        return Err(SchemaError::new(path, Problem::TooShort { min }));
    }
    if n > max {
        return Err(SchemaError::new(path, Problem::TooLong { max }));
    }
    Ok(s.to_owned())
}

fn uint(v: &Value, path: &str, min: u64, max: u64) -> R<u64> {
    let Value::Number(n) = v else {
        return Err(SchemaError::new(path, Problem::WrongType { expected: "integer" }));
    };
    match n.as_u64() {
        Some(u) if (min..=max).contains(&u) => Ok(u),
        Some(_) => Err(SchemaError::new(path, Problem::OutOfRange { min, max })),
        None if n.is_i64() => Err(SchemaError::new(path, Problem::OutOfRange { min, max })),
        None => Err(SchemaError::new(path, Problem::WrongType { expected: "integer" })),
    }
}

fn check_url(s: &str, path: &str) -> R<()> {
    weburl::validate_https_url(s).map_err(|detail| SchemaError::new(path, Problem::BadUrl { detail }))
}

/// True if `s` is two ASCII uppercase letters (ISO 3166-1 alpha-2 syntax).
pub fn is_country_code(s: &str) -> bool {
    s.len() == 2 && s.bytes().all(|b| b.is_ascii_uppercase())
}

fn check_country(s: &str, path: &str) -> R<()> {
    if is_country_code(s) { Ok(()) } else { Err(SchemaError::new(path, Problem::BadCountry)) }
}

/// Checks a `product.category`: 1–200 characters; `/`-separated segments,
/// each non-empty and without leading or trailing ASCII whitespace; no ASCII
/// uppercase letters. Non-ASCII letters are not case-checked, so the rule
/// does not depend on the Unicode version.
pub fn is_valid_category(s: &str) -> bool {
    let n = s.chars().count();
    (1..=200).contains(&n)
        && !s.bytes().any(|b| b.is_ascii_uppercase())
        && s.split('/').all(|seg| !seg.is_empty() && product_key::trim_ascii_ws(seg).len() == seg.len())
}

/// Parses a body by kind and checks every rule of ARCHITECTURE 4.5 that the
/// body alone determines. Unknown kinds return [`Body::Unknown`].
///
/// Rules that need the record header ([`check_record`]) or other records
/// (views) are not checked here.
pub fn parse_body(kind: &str, body: &Map<String, Value>) -> Result<Body, SchemaError> {
    let o = Obj::root(body);
    Ok(match kind {
        names::PROFILE => Body::Profile(Profile {
            name: o.str("name", 1, 100)?,
            role: o.en("role", ROLES, Role::parse)?,
            url: o.opt_url("url")?,
            about: o.opt_str("about", 0, 2000)?,
        }),
        names::ATTEST_IDENTITY => {
            let level = o.en("level", LEVELS, Level::parse)?;
            let legal_name = o.opt_str("legal_name", 1, 200)?;
            if legal_name.is_some() && level != Level::Business {
                return Err(SchemaError::new("legal_name", Problem::OnlyForBusiness));
            }
            Body::AttestIdentity(AttestIdentity {
                subject: o.author("subject")?,
                person: Hex32(o.hex32("person")?),
                level,
                legal_name,
                country: o.opt_country("country")?,
                expires: o.time("expires")?,
            })
        }
        names::ATTEST_DOMAIN => {
            let domain = o.raw_str("domain")?;
            weburl::validate_hostname(domain)
                .map_err(|detail| SchemaError::new("domain", Problem::BadUrl { detail }))?;
            Body::AttestDomain(AttestDomain { subject: o.author("subject")?, domain: domain.to_owned(), expires: o.time("expires")? })
        }
        names::ATTEST_REVOKE => Body::AttestRevoke(AttestRevoke {
            subject: o.author("subject")?,
            valid_through_seq: o.uint("valid_through_seq", 0, MAX_SAFE_INTEGER)?,
        }),
        names::PRODUCT => parse_product(&o)?,
        names::LISTING => {
            let ships_to = match o.opt_arr("ships_to", 250)? {
                None => None,
                Some(a) => {
                    let mut v = Vec::with_capacity(a.len());
                    for (i, x) in a.iter().enumerate() {
                        let p = format!("ships_to[{i}]");
                        let s = as_str(x, &p)?;
                        check_country(s, &p)?;
                        v.push(s.to_owned());
                    }
                    Some(v)
                }
            };
            let location = match o.opt_obj("location")? {
                None => None,
                Some(l) => Some(Location { country: l.country("country")?, region: l.opt_str("region", 0, 100)? }),
            };
            Body::Listing(Listing {
                product: o.product_key("product")?,
                url: o.url("url")?,
                price: o.price("price")?,
                availability: o.en("availability", AVAILABILITIES, Availability::parse)?,
                condition: o.en("condition", CONDITIONS, Condition::parse)?,
                title: o.opt_str("title", 0, 300)?,
                ships_to,
                location,
            })
        }
        names::OBSERVATION => Body::Observation(Observation {
            product: o.product_key("product")?,
            url: o.url("url")?,
            price: o.price("price")?,
            observed_at: o.time("observed_at")?,
            listing: o.opt_id("listing")?,
            availability: o.opt_en("availability", AVAILABILITIES, Availability::parse)?,
        }),
        names::REVIEW => {
            let receipt = match o.get("receipt")? {
                None => None,
                Some(v) => Some(
                    receipt::parse_receipt(v)
                        .map_err(|detail| SchemaError::new("receipt", Problem::BadReceipt { detail }))?,
                ),
            };
            Body::Review(Review {
                product: o.product_key("product")?,
                rating: o.uint("rating", 1, 5)? as u8,
                text: o.str("text", 1, 10_000)?,
                title: o.opt_str("title", 0, 200)?,
                listing: o.opt_id("listing")?,
                receipt,
            })
        }
        names::FLAG => Body::Flag(Flag {
            target: o.id("target")?,
            reason: o.en("reason", FLAG_REASONS, FlagReason::parse)?,
            note: o.opt_str("note", 0, 2000)?,
        }),
        names::APPEAL => Body::Appeal(Appeal { target: o.id("target")?, note: o.str("note", 1, 2000)? }),
        names::RETRACT => Body::Retract(Retract { target: o.id("target")? }),
        names::RETIRE => Body::Retire(Retire { successor: o.opt_hex32("successor")?.map(AuthorId) }),
        names::REDACT => Body::Redact(Redact {
            target: o.id("target")?,
            reason: o.en("reason", REDACT_REASONS, RedactReason::parse)?,
            note: o.opt_str("note", 0, 2000)?,
        }),
        _ => Body::Unknown(body.clone()),
    })
}

fn parse_product(o: &Obj<'_>) -> R<Body> {
    let gtin = match o.opt_raw_str("gtin")? {
        None => None,
        Some(g) => {
            product_key::normalize_gtin(g).map_err(|detail| SchemaError::new("gtin", Problem::BadGtin { detail }))?;
            Some(g.to_owned())
        }
    };
    let brand = o.str("brand", 1, 100)?;
    let mpn = o.opt_str("mpn", 1, 100)?;
    if let (None, Some(m)) = (&gtin, &mpn) {
        // The mpn: key must be derivable (brand and MPN not blank after trimming).
        ProductKey::from_brand_mpn(&brand, m)
            .map_err(|detail| SchemaError::new("mpn", Problem::BadProductKey { detail }))?;
    }
    let category = match o.opt_raw_str("category")? {
        None => None,
        Some(c) => {
            if c.chars().count() > 200 {
                return Err(SchemaError::new("category", Problem::TooLong { max: 200 }));
            }
            if !is_valid_category(c) {
                return Err(SchemaError::new("category", Problem::BadCategory));
            }
            Some(c.to_owned())
        }
    };
    let attributes = match o.opt_obj("attributes")? {
        None => None,
        Some(a) => {
            if a.m.len() > 64 {
                return Err(SchemaError::new("attributes", Problem::TooMany { max: 64 }));
            }
            let mut out = BTreeMap::new();
            for (k, v) in a.m {
                let p = format!("attributes.{k}");
                let n = k.chars().count();
                if n < 1 {
                    return Err(SchemaError::new(p, Problem::TooShort { min: 1 }));
                }
                if n > 64 {
                    return Err(SchemaError::new(p, Problem::TooLong { max: 64 }));
                }
                out.insert(k.clone(), text(v, &p, 0, 500)?);
            }
            Some(out)
        }
    };
    let images = match o.opt_arr("images", 16)? {
        None => None,
        Some(arr) => {
            let mut v = Vec::with_capacity(arr.len());
            for (i, x) in arr.iter().enumerate() {
                let p = format!("images[{i}]");
                let Value::Object(m) = x else {
                    return Err(SchemaError::new(p, Problem::WrongType { expected: "object" }));
                };
                let io = Obj { m, path: p };
                v.push(Image { url: io.url("url")?, sha256: io.opt_hex32("sha256")?.map(Hex32) });
            }
            Some(v)
        }
    };
    Ok(Body::Product(Product {
        name: o.str("name", 1, 300)?,
        brand,
        gtin,
        mpn,
        category,
        attributes,
        images,
        description: o.opt_str("description", 0, 5000)?,
    }))
}

/// Parses a record's body and checks the rules that also need its header:
///
/// - the body is present ([`Problem::BodyMissing`] otherwise);
/// - a known non-replaceable kind carries no `replaces`
///   ([`Problem::NotReplaceable`], field `"replaces"`);
/// - `observation.observed_at` ≤ the record's `ts` ([`Problem::AfterRecordTs`]).
///
/// Rules that need other records (references, fixed fields, permissions,
/// `retract` ordering) are left to views.
pub fn check_record(record: &Record) -> Result<Body, SchemaError> {
    let Some(map) = record.body() else {
        return Err(SchemaError::new("body", Problem::BodyMissing));
    };
    let body = parse_body(record.kind(), map)?;
    if record.replaces().is_some() && kind_info(record.kind()).is_some_and(|k| !k.replaceable) {
        return Err(SchemaError::new("replaces", Problem::NotReplaceable));
    }
    if let Body::Observation(ob) = &body
        && ob.observed_at > record.ts()
    {
        return Err(SchemaError::new("observed_at", Problem::AfterRecordTs));
    }
    Ok(body)
}

/// The product key of a `product` body (ARCHITECTURE 4.8), given the root
/// ID of its chain. Fails if the body is not a valid product body.
pub fn derive_product_key(body: &Map<String, Value>, root: RecordId) -> Result<ProductKey, SchemaError> {
    match parse_body(names::PRODUCT, body)? {
        Body::Product(p) => Ok(p.key(root)),
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests;
