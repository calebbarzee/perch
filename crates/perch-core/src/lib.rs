//! # perch-core
//!
//! Perch records and everything needed to check them in isolation
//! ([ARCHITECTURE](../../../ARCHITECTURE.md) sections 4 and 5.2).
//!
//! | Module | Contents |
//! |---|---|
//! | [`ids`] | `AuthorId`, `RecordId`, `Hex32`, `Sig`: fixed-size lowercase-hex values |
//! | [`json`] | Strict JSON parser: no duplicate keys, integers only, no lone surrogates, depth limit |
//! | [`canonical`] | Canonical JSON encoder (RFC 8785 restricted to integers) |
//! | [`keys`] | `Keypair`, SHA-256, strict Ed25519 verification |
//! | [`record`] | `Record` (isolated-valid signed record), `Header`, `sign_record`, JSONL helpers |
//! | [`receipt`] | Detached receipts: `sign_receipt`, `verify_receipt` |
//! | [`kinds`] | Typed bodies, `parse_body`, `check_record`, kind metadata (replaceable, fixed fields, control kinds) |
//! | [`product_key`] | `ProductKey`, GTIN validation |
//! | [`weburl`] | `URL` validation (WHATWG, https, domain host), hostname rules, domain matching |
//! | [`currency`] | ISO 4217 table |
//! | [`fork`] | `EvidencePair` and its total order, `next_position` |
//! | [`limits`] | Size limits and signature domains |
//!
//! Everything here compiles to `wasm32-unknown-unknown`. The only
//! platform-specific item is [`Keypair::generate`], which exists on native
//! targets only.
//!
//! ## Example
//!
//! ```
//! use perch_core::{Keypair, Record, Unsigned, sign_record};
//! use serde_json::json;
//!
//! let kp = Keypair::from_seed([1; 32]);
//! let body = json!({"name": "Ada", "role": "buyer"}).as_object().unwrap().clone();
//! let rec = sign_record(&kp, Unsigned { seq: 1, prev: None, ts: 1_760_000_000_000,
//!                                      kind: "profile".into(), replaces: None, body }).unwrap();
//! let line = rec.to_json_string();             // canonical wire form = one JSONL line
//! let parsed = Record::from_json_str(&line).unwrap(); // strict parse + verify
//! assert_eq!(parsed.id(), rec.id());
//! let typed = perch_core::kinds::check_record(&parsed).unwrap(); // semantic body check
//! assert!(matches!(typed, perch_core::kinds::Body::Profile(_)));
//! ```

#![warn(missing_docs)]

pub mod canonical;
pub mod currency;
pub mod fork;
pub mod ids;
pub mod json;
pub mod keys;
pub mod kinds;
pub mod limits;
pub mod product_key;
pub mod receipt;
pub mod record;
pub mod weburl;

pub use canonical::{CanonicalError, to_canonical_json};
pub use fork::{EvidencePair, ForkError, ForkType, next_position};
pub use ids::{AuthorId, Hex32, HexError, RecordId, Sig};
pub use json::JsonError;
pub use keys::{Keypair, SignatureError, sha256};
pub use kinds::{
    Body, SchemaError, check_record, fixed_fields, is_control_kind, is_replaceable, kind_info, parse_body,
};
pub use product_key::{ProductKey, ProductKeyError};
pub use receipt::{Receipt, ReceiptError, sign_receipt, verify_receipt};
pub use record::{Header, Record, RecordError, Unsigned, body_hash, parse_jsonl, sign_record, to_jsonl};
pub use weburl::{UrlError, https_url_host};

/// Re-export of `serde_json`, so dependents use the same version.
pub use serde_json;
