//! View result shapes (ARCHITECTURE 7.8). Field names are part of the
//! contract.

use std::collections::BTreeMap;

use perch_core::{AuthorId, ProductKey, RecordId};
use serde::Serialize;

use crate::num::Num;

/// Who said a value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Source {
    /// Author of the record.
    pub author: AuthorId,
    /// The record (latest version of its chain).
    pub record: RecordId,
    /// Author is verified.
    pub verified: bool,
}

/// One value with its sources, and the values it beat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fact<T> {
    /// The value shown.
    pub value: T,
    /// Sources of `value`.
    pub sources: Vec<Source>,
    /// Other values, in rank order.
    pub conflicts: Vec<Conflict<T>>,
}

/// A losing value of a [`Fact`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Conflict<T> {
    /// The value.
    pub value: T,
    /// Its sources.
    pub sources: Vec<Source>,
}

/// An appeal by the target's author.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Appeal {
    /// Latest version of the appeal chain.
    pub record: RecordId,
    /// Appeal text.
    pub note: String,
    /// `ts` of that version.
    pub ts: u64,
}

/// A body-removal notice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Redaction {
    /// The `redact` record, or `null` when no notice exists.
    pub record: Option<RecordId>,
    /// Its author.
    pub by: Option<AuthorId>,
    /// Its reason.
    pub reason: Option<String>,
    /// Its note.
    pub note: Option<String>,
    /// The product the notice names. Stated by the notice's author (a node
    /// operator), not by the removed record.
    pub product: Option<ProductKey>,
}

/// Author status (ARCHITECTURE 7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorStatus {
    /// No cutoff.
    Ok,
    /// Fork evidence and no counting revoke.
    Forked,
    /// Retired.
    Retired,
    /// A counting `attest.revoke`.
    Revoked,
}

/// Display status of a chain (docs/views.md section 3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Shown.
    Visible,
    /// Collapsed by flags.
    Hidden,
    /// Withdrawn by its author.
    Retracted,
    /// Latest version has no body.
    Unavailable,
}

impl Status {
    /// The JSON string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Visible => "visible",
            Status::Hidden => "hidden",
            Status::Retracted => "retracted",
            Status::Unavailable => "unavailable",
        }
    }
}

/// Flag summary of a chain.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Flags {
    /// Used live flags, one per author.
    pub count: u64,
    /// Flags that count (one per person).
    pub counted: u64,
    /// Reason → number of used live flags.
    pub reasons: BTreeMap<String, u64>,
}

/// Moderation state of a chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Moderation {
    /// Display status.
    pub status: Status,
    /// Flags.
    pub flags: Flags,
    /// Appeal.
    pub appeal: Option<Appeal>,
    /// Redaction notice when the latest version has no body.
    pub redaction: Option<Redaction>,
}

/// `AuthorView.verified`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Verified {
    /// `person` or `business`.
    pub level: String,
    /// Registered name of a business.
    pub legal_name: Option<String>,
    /// ISO 3166-1 alpha-2.
    pub country: Option<String>,
    /// Attestor.
    pub attestor: AuthorId,
    /// Expiry.
    pub expires: u64,
}

/// A counting domain attestation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct DomainAttestation {
    /// Domain.
    pub domain: String,
    /// Attestor.
    pub attestor: AuthorId,
    /// Expiry.
    pub expires: u64,
}

/// `AuthorView.restriction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RestrictionView {
    /// `hidden_listings` when the counts below explain the state;
    /// `forked_key` when the person is restricted only because a key forked.
    pub reason: &'static str,
    /// Restriction-hidden listings of the person.
    pub hidden_listings: u64,
    /// Distinct persons with counted flags on them.
    pub distinct_flaggers: u64,
}

/// `AuthorView.revoked`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Revoked {
    /// Revoking attestor.
    pub attestor: AuthorId,
    /// Last valid seq.
    pub valid_through_seq: u64,
}

/// `AuthorView.retired`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Retired {
    /// Seq of the `retire` record.
    pub seq: u64,
    /// Named successor.
    pub successor: Option<AuthorId>,
}

/// `AuthorView.seller_reviews`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SellerReviews {
    /// Counted reviews of products this author lists.
    pub total: u64,
    /// Of those, reviews citing a valid receipt issued by this author.
    pub with_valid_receipt: u64,
}

/// One author.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthorView {
    /// Author ID.
    pub id: AuthorId,
    /// Profile name.
    pub name: Option<String>,
    /// Profile role.
    pub role: Option<String>,
    /// Profile URL.
    pub url: Option<String>,
    /// Profile text.
    pub about: Option<String>,
    /// Moderation of the profile chain.
    pub profile: Option<Moderation>,
    /// Profile chain status (`null` without a profile).
    pub profile_status: Option<Status>,
    /// Identity verification.
    pub verified: Option<Verified>,
    /// Counting domain attestations.
    pub domains: Vec<DomainAttestation>,
    /// Status.
    pub status: AuthorStatus,
    /// Restricted (per person).
    pub restricted: bool,
    /// Restriction counts.
    pub restriction: Option<RestrictionView>,
    /// Counting revoke.
    pub revoked: Option<Revoked>,
    /// Fork seq.
    pub fork_seq: Option<u64>,
    /// Evidence pair IDs.
    pub fork_evidence: Option<[RecordId; 2]>,
    /// Retirement.
    pub retired: Option<Retired>,
    /// Reviews of products this author lists.
    pub seller_reviews: SellerReviews,
    /// Active records.
    pub record_count: u64,
    /// Listing chains with status hidden.
    pub hidden_listing_count: u64,
}

/// A price.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Price {
    /// Minor units.
    pub amount: u64,
    /// ISO 4217 code.
    pub currency: String,
}

/// `ListingView.location`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Location {
    /// Country.
    pub country: String,
    /// Region.
    pub region: Option<String>,
}

/// One price history entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PricePoint {
    /// Version `ts` or `observed_at`.
    pub ts: u64,
    /// Minor units.
    pub amount: u64,
    /// Currency.
    pub currency: String,
    /// `seller` or `observation`.
    pub source: &'static str,
    /// Record.
    pub record: RecordId,
}

/// One listing chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListingView {
    /// Root ID.
    pub id: RecordId,
    /// Latest version.
    pub latest: RecordId,
    /// Seller.
    pub seller: AuthorId,
    /// Seller's profile name (visible profiles only).
    pub seller_name: Option<String>,
    /// Seller verified.
    pub seller_verified: bool,
    /// Seller person restricted.
    pub seller_restricted: bool,
    /// Seller status.
    pub seller_status: AuthorStatus,
    /// URL host matches a counting domain attestation of the seller.
    pub domain_verified: bool,
    /// Counts toward `price_range` and `min_price` (8.3).
    pub feeds_prices: bool,
    /// First price-feed rule the listing fails: `hidden`, `retracted`,
    /// `unavailable`, `availability`, `seller_restricted`, or `domain_unverified`.
    pub price_excluded_reason: Option<&'static str>,
    /// The matching domain attestation.
    pub domain: Option<DomainAttestation>,
    /// Product key.
    pub product: ProductKey,
    /// Title.
    pub title: Option<String>,
    /// URL.
    pub url: Option<String>,
    /// Current price.
    pub price: Option<Price>,
    /// Availability.
    pub availability: Option<String>,
    /// Condition.
    pub condition: Option<String>,
    /// Shipping countries.
    pub ships_to: Vec<String>,
    /// Seller location.
    pub location: Option<Location>,
    /// Latest version `ts`.
    pub updated_ts: u64,
    /// Display status.
    pub status: Status,
    /// Flags.
    pub flags: Flags,
    /// An appeal exists.
    pub appealed: bool,
    /// The appeal.
    pub appeal: Option<Appeal>,
    /// Redaction notice.
    pub redaction: Option<Redaction>,
    /// Price history.
    pub price_history: Vec<PricePoint>,
}

/// One review chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewView {
    /// Root ID.
    pub id: RecordId,
    /// Latest version.
    pub latest: RecordId,
    /// Author.
    pub author: AuthorId,
    /// Author's profile name.
    pub author_name: Option<String>,
    /// Author verified.
    pub author_verified: bool,
    /// Author status.
    pub author_status: AuthorStatus,
    /// `purchase == "verified"`.
    pub verified_purchase: bool,
    /// Verified-purchase outcome.
    pub purchase: &'static str,
    /// Counts in ratings.
    pub counted: bool,
    /// Why not.
    pub excluded_reason: Option<&'static str>,
    /// Rating.
    pub rating: Option<u8>,
    /// Title.
    pub title: Option<String>,
    /// Text.
    pub text: Option<String>,
    /// Latest version `ts`.
    pub ts: u64,
    /// Listing root.
    pub listing: Option<RecordId>,
    /// Display status.
    pub status: Status,
    /// Flags.
    pub flags: Flags,
    /// Appeal.
    pub appeal: Option<Appeal>,
    /// Redaction notice.
    pub redaction: Option<Redaction>,
}

/// Count, mean, histogram.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RatingStats {
    /// Reviews.
    pub count: u64,
    /// Mean rating.
    pub mean: Option<Num>,
    /// `[n1 … n5]`.
    pub histogram: [u64; 5],
}

/// Excluded review counts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Excluded {
    /// Reserved; 0 in v1.
    pub hidden: u64,
    /// Latest version has no body.
    pub unavailable: u64,
    /// Retracted.
    pub retracted: u64,
    /// By a seller of the product.
    pub seller: u64,
    /// Author forked.
    pub forked: u64,
    /// Ignored by a revoke.
    pub revoked: u64,
    /// Not the latest review of the author or person.
    pub superseded: u64,
}

/// Rating summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RatingSummary {
    /// Counted reviews.
    pub count: u64,
    /// Mean.
    pub mean: Option<Num>,
    /// Histogram.
    pub histogram: [u64; 5],
    /// Verified purchases.
    pub verified_purchase: RatingStats,
    /// Verified authors.
    pub verified_identity: RatingStats,
    /// Excluded counts.
    pub excluded: Excluded,
}

/// A product image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Image {
    /// URL.
    pub url: String,
    /// SHA-256 of the file.
    pub sha256: Option<String>,
    /// Some author of the image is verified.
    pub author_verified: bool,
}

/// Price statistics of one currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PriceStats {
    /// Minimum.
    pub min: Option<u64>,
    /// Maximum.
    pub max: Option<u64>,
    /// Lower median.
    pub median: Option<u64>,
    /// Feeding listings.
    pub count: u64,
    /// Listings excluded only because their domain is not verified.
    pub excluded_unverified_domain: u64,
}

/// Product chains not used as fact sources.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ExcludedSources {
    /// Hidden while visible chains exist.
    pub hidden: u64,
    /// Retracted.
    pub retracted: u64,
    /// No body.
    pub unavailable: u64,
    /// Author forked.
    pub forked: u64,
    /// Ignored by a revoke.
    pub revoked: u64,
}

/// One product.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProductView {
    /// Product key.
    pub key: ProductKey,
    /// Moderation over all product chains.
    pub moderation: Moderation,
    /// Name.
    pub name: Fact<String>,
    /// Brand.
    pub brand: Fact<String>,
    /// Category.
    pub category: Option<Fact<String>>,
    /// Description.
    pub description: Option<Fact<String>>,
    /// GTIN (14 digits).
    pub gtin: Option<String>,
    /// MPN.
    pub mpn: Option<String>,
    /// Attributes.
    pub attributes: BTreeMap<String, Fact<String>>,
    /// Images.
    pub images: Vec<Image>,
    /// Listings.
    pub listings: Vec<ListingView>,
    /// Reviews.
    pub reviews: Vec<ReviewView>,
    /// Ratings.
    pub ratings: RatingSummary,
    /// Currency → price statistics.
    pub price_range: BTreeMap<String, PriceStats>,
    /// Product chains not used as sources.
    pub excluded_sources: ExcludedSources,
}

/// One search result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchHit {
    /// Product key.
    pub key: ProductKey,
    /// Name.
    pub name: String,
    /// Brand.
    pub brand: String,
    /// Category.
    pub category: Option<String>,
    /// Score.
    pub score: Num,
    /// Every score component.
    pub why: Vec<String>,
    /// Product moderation status.
    pub moderation_status: Status,
    /// Mean of verified-identity reviews.
    pub rating_mean: Option<Num>,
    /// Verified-identity reviews.
    pub rating_count: u64,
    /// Dirichlet-smoothed verified-identity mean.
    pub rating_smoothed: Num,
    /// Verified purchases.
    pub verified_purchase_count: u64,
    /// Their mean.
    pub verified_purchase_mean: Option<Num>,
    /// Lowest feeding price.
    pub min_price: Option<Price>,
}

/// Store counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Stats {
    /// Records held, including evidence records.
    pub records: u64,
    /// Distinct authors.
    pub authors: u64,
    /// Distinct product keys of product chains.
    pub products: u64,
    /// Listing chains.
    pub listings: u64,
    /// Review chains.
    pub reviews: u64,
    /// Forked authors.
    pub forks: u64,
    /// Semantically invalid records.
    pub invalid: u64,
    /// Records without a body.
    pub redacted: u64,
    /// Records of unknown kinds.
    pub unknown: u64,
}

/// One flag on a chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FlagView {
    /// Latest version of the flag chain.
    pub record: RecordId,
    /// Flagger.
    pub author: AuthorId,
    /// Flagger's profile name.
    pub author_name: Option<String>,
    /// Flagger verified.
    pub author_verified: bool,
    /// Reason.
    pub reason: Option<String>,
    /// Note.
    pub note: Option<String>,
    /// Counts.
    pub counted: bool,
    /// Why not.
    pub excluded_reason: Option<&'static str>,
    /// Latest version `ts`.
    pub ts: u64,
}

/// Summary of one chain of an author.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChainSummary {
    /// Root ID.
    pub root: RecordId,
    /// Latest version.
    pub latest: RecordId,
    /// Kind.
    pub kind: String,
    /// Latest `ts`.
    pub ts: u64,
    /// `visible`, `hidden`, `retracted`, `unavailable`, or `invalid`.
    pub status: &'static str,
    /// Product key.
    pub product: Option<String>,
    /// Title or name.
    pub title: Option<String>,
}

/// Validity of one record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordStatus {
    /// Semantically valid.
    pub semantic_valid: bool,
    /// Reason code when not valid.
    pub reason: Option<String>,
    /// Root of its chain.
    pub root: RecordId,
    /// Candidate versions of the chain.
    pub versions: Vec<RecordId>,
    /// Why views ignore an otherwise valid record.
    pub ignored: Option<&'static str>,
}
