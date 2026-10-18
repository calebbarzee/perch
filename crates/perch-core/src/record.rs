//! Signed records: header, signature, optional body (ARCHITECTURE 4.1–4.4).
//!
//! A [`Record`] value is always **isolated-valid**: every constructor checks
//! fields, types, sizes, the `kind` format, `body_hash` (when the body is
//! present), and the signature. Log and semantic validity are checked
//! elsewhere (`perch-store`, `perch-views`, and [`crate::kinds`]).
//!
//! A record keeps its body exactly as received (a JSON object, including
//! fields this software does not understand) and never re-encodes it from a
//! typed struct.

use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::canonical::{self, CanonicalError};
use crate::ids::{AuthorId, Hex32, HexError, RecordId, Sig};
use crate::json::{self, JsonError};
use crate::keys::{self, Keypair, SignatureError};
use crate::limits::{MAX_BODY_BYTES, MAX_DEPTH, MAX_HEADER_BYTES, MAX_LINE_BYTES, MAX_SAFE_INTEGER, RECORD_DOMAIN};

/// The only record format version.
pub const VERSION: u64 = 1;

/// Why a record is not isolated-valid. Serializes as
/// `{ "error": "<snake_case variant>", ...fields }`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum RecordError {
    /// The line is longer than [`MAX_LINE_BYTES`]; checked before parsing.
    #[error("record is {actual} bytes; limit is {limit}")]
    TooLarge {
        /// Size in bytes.
        actual: usize,
        /// Limit in bytes.
        limit: usize,
    },
    /// Strict JSON parsing failed.
    #[error("{detail}")]
    Json {
        /// The parse error.
        detail: JsonError,
    },
    /// The record is not a JSON object.
    #[error("record is not a JSON object")]
    NotObject,
    /// A top-level field that is not part of the format.
    #[error("unknown field {field:?}")]
    UnknownField {
        /// Field name.
        field: String,
    },
    /// A required field is absent.
    #[error("missing field {field}")]
    MissingField {
        /// Field name.
        field: &'static str,
    },
    /// A field has the wrong JSON type or an invalid value.
    #[error("field {field}: {problem}")]
    InvalidField {
        /// Field name.
        field: &'static str,
        /// What is wrong.
        problem: &'static str,
    },
    /// `v` is not 1.
    #[error("unsupported version {version}")]
    Version {
        /// The value of `v`.
        version: i64,
    },
    /// `prev` is `null` for `seq > 1`, or not `null` for `seq = 1`.
    #[error("prev must be null if and only if seq is 1")]
    PrevMismatch,
    /// `kind` does not match `^[a-z][a-z0-9_.]{0,31}$`.
    #[error("kind {kind:?} does not match ^[a-z][a-z0-9_.]{{0,31}}$")]
    BadKind {
        /// The value of `kind`.
        kind: String,
    },
    /// `body` is present but not a JSON object.
    #[error("body is not a JSON object")]
    BodyNotObject,
    /// The canonical header is larger than [`MAX_HEADER_BYTES`].
    #[error("canonical header is {actual} bytes; limit is {limit}")]
    HeaderTooLarge {
        /// Size in bytes.
        actual: usize,
        /// Limit in bytes.
        limit: usize,
    },
    /// The canonical body is larger than [`MAX_BODY_BYTES`].
    #[error("canonical body is {actual} bytes; limit is {limit}")]
    BodyTooLarge {
        /// Size in bytes.
        actual: usize,
        /// Limit in bytes.
        limit: usize,
    },
    /// The record nests deeper than [`MAX_DEPTH`].
    #[error("nesting depth exceeds {max}")]
    TooDeep {
        /// Allowed depth.
        max: usize,
    },
    /// The body has no canonical encoding (for example a float).
    #[error("body: {detail}")]
    Canonical {
        /// The encoding error.
        detail: CanonicalError,
    },
    /// `sha256(canonical body)` differs from `body_hash`.
    #[error("body does not match body_hash")]
    BodyHashMismatch,
    /// The signature does not verify.
    #[error("signature: {detail}")]
    Signature {
        /// The verification error.
        detail: SignatureError,
    },
    /// A body was supplied for a different record.
    #[error("body belongs to record {other}, not {this}")]
    WrongRecord {
        /// This record's ID.
        this: RecordId,
        /// The other record's ID.
        other: RecordId,
    },
}

impl From<JsonError> for RecordError {
    fn from(e: JsonError) -> Self {
        match e {
            JsonError::TooLarge { actual, limit } => RecordError::TooLarge { actual, limit },
            JsonError::TooDeep { max, .. } => RecordError::TooDeep { max },
            detail => RecordError::Json { detail },
        }
    }
}

/// The signed header of a record (fields marked "header" in ARCHITECTURE 4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Header {
    /// Format version; always [`VERSION`].
    pub v: u64,
    /// The author's public key.
    pub author: AuthorId,
    /// Position in the author's log, starting at 1.
    pub seq: u64,
    /// ID of the author's record at `seq - 1`; `None` iff `seq == 1`.
    pub prev: Option<RecordId>,
    /// Time claimed by the author, Unix milliseconds.
    pub ts: u64,
    /// Record type.
    pub kind: String,
    /// ID of an earlier record this one supersedes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaces: Option<RecordId>,
    /// SHA-256 of the canonical body.
    pub body_hash: Hex32,
}

impl Header {
    /// Canonical JSON of the header. Keys in UTF-16 order (all ASCII here):
    /// author, body_hash, kind, prev, replaces, seq, ts, v.
    pub fn to_canonical(&self) -> String {
        let mut s = String::with_capacity(320 + self.kind.len());
        s.push_str("{\"author\":\"");
        s.push_str(&self.author.to_hex());
        s.push_str("\",\"body_hash\":\"");
        s.push_str(&self.body_hash.to_hex());
        s.push_str("\",\"kind\":");
        canonical::write_string(&self.kind, &mut s);
        s.push_str(",\"prev\":");
        push_opt_id(&mut s, self.prev);
        if let Some(r) = self.replaces {
            s.push_str(",\"replaces\":\"");
            s.push_str(&r.to_hex());
            s.push('"');
        }
        s.push_str(",\"seq\":");
        s.push_str(&self.seq.to_string());
        s.push_str(",\"ts\":");
        s.push_str(&self.ts.to_string());
        s.push_str(",\"v\":");
        s.push_str(&self.v.to_string());
        s.push('}');
        s
    }

    /// Checks the rules that do not need the signature or body.
    fn check(&self) -> Result<(), RecordError> {
        if self.v != VERSION {
            return Err(RecordError::Version { version: self.v as i64 });
        }
        if self.seq < 1 || self.seq > MAX_SAFE_INTEGER {
            return Err(RecordError::InvalidField { field: "seq", problem: "must be an integer from 1 to 2^53 - 1" });
        }
        if self.ts > MAX_SAFE_INTEGER {
            return Err(RecordError::InvalidField { field: "ts", problem: "must be an integer from 0 to 2^53 - 1" });
        }
        if (self.seq == 1) != self.prev.is_none() {
            return Err(RecordError::PrevMismatch);
        }
        if !is_valid_kind(&self.kind) {
            return Err(RecordError::BadKind { kind: self.kind.clone() });
        }
        Ok(())
    }
}

fn push_opt_id(s: &mut String, id: Option<RecordId>) {
    match id {
        Some(id) => {
            s.push('"');
            s.push_str(&id.to_hex());
            s.push('"');
        }
        None => s.push_str("null"),
    }
}

/// True if `kind` matches `^[a-z][a-z0-9_.]{0,31}$`.
pub fn is_valid_kind(kind: &str) -> bool {
    let b = kind.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && b[0].is_ascii_lowercase()
        && b[1..].iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'.')
}

#[derive(Debug)]
struct BodyData {
    map: Map<String, Value>,
    canonical: String,
}

#[derive(Debug)]
struct Inner {
    header: Header,
    id: RecordId,
    sig: Sig,
    header_canonical: String,
    body: Option<Arc<BodyData>>,
}

/// An isolated-valid signed record. Cloning is cheap (reference counted).
///
/// Equality compares the ID and whether the body is present.
#[derive(Clone)]
pub struct Record {
    inner: Arc<Inner>,
}

impl core::fmt::Debug for Record {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Record")
            .field("id", &self.inner.id)
            .field("author", &self.inner.header.author)
            .field("seq", &self.inner.header.seq)
            .field("kind", &self.inner.header.kind)
            .field("has_body", &self.has_body())
            .finish()
    }
}

impl PartialEq for Record {
    fn eq(&self, other: &Self) -> bool {
        self.inner.id == other.inner.id && self.has_body() == other.has_body()
    }
}
impl Eq for Record {}

/// The content of a record to be signed by [`sign_record`].
#[derive(Debug, Clone, PartialEq)]
pub struct Unsigned {
    /// Position in the author's log, starting at 1.
    pub seq: u64,
    /// ID of the author's record at `seq - 1`; `None` iff `seq == 1`.
    pub prev: Option<RecordId>,
    /// Time claimed by the author, Unix milliseconds.
    pub ts: u64,
    /// Record type, for example `"review"`.
    pub kind: String,
    /// ID of an earlier record this one supersedes.
    pub replaces: Option<RecordId>,
    /// Kind-specific content.
    pub body: Map<String, Value>,
}

/// Signs a record. Checks the same isolated rules as parsing, except that
/// the signature is produced rather than verified. Body schemas are not
/// checked; use [`crate::kinds::parse_body`] first if needed.
pub fn sign_record(keypair: &Keypair, unsigned: Unsigned) -> Result<Record, RecordError> {
    let Unsigned { seq, prev, ts, kind, replaces, body } = unsigned;
    let body = BodyData::new(body)?;
    let header =
        Header { v: VERSION, author: keypair.author(), seq, prev, ts, kind, replaces, body_hash: body.hash() };
    header.check()?;
    let header_canonical = header.to_canonical();
    if header_canonical.len() > MAX_HEADER_BYTES {
        return Err(RecordError::HeaderTooLarge { actual: header_canonical.len(), limit: MAX_HEADER_BYTES });
    }
    let sig = keypair.sign_with_domain(RECORD_DOMAIN, header_canonical.as_bytes());
    let id = RecordId(keys::sha256(header_canonical.as_bytes()));
    Ok(Record { inner: Arc::new(Inner { header, id, sig, header_canonical, body: Some(Arc::new(body)) }) })
}

impl BodyData {
    fn new(map: Map<String, Value>) -> Result<Self, RecordError> {
        let canonical = body_canonical(&map)?;
        Ok(BodyData { map, canonical })
    }

    fn hash(&self) -> Hex32 {
        keys::sha256_hex32(self.canonical.as_bytes())
    }
}

fn body_canonical(map: &Map<String, Value>) -> Result<String, RecordError> {
    let max_body_depth = MAX_DEPTH - 1;
    let mut d = 1usize;
    for v in map.values() {
        d = d.max(1 + json::depth(v, max_body_depth));
        if d > max_body_depth {
            return Err(RecordError::TooDeep { max: MAX_DEPTH });
        }
    }
    let canonical = canonical::map_to_canonical_json(map).map_err(|e| match e {
        CanonicalError::TooDeep { .. } => RecordError::TooDeep { max: MAX_DEPTH },
        detail => RecordError::Canonical { detail },
    })?;
    if canonical.len() > MAX_BODY_BYTES {
        return Err(RecordError::BodyTooLarge { actual: canonical.len(), limit: MAX_BODY_BYTES });
    }
    Ok(canonical)
}

/// Computes `body_hash` for a body: SHA-256 of its canonical JSON.
pub fn body_hash(body: &Map<String, Value>) -> Result<Hex32, RecordError> {
    Ok(keys::sha256_hex32(body_canonical(body)?.as_bytes()))
}

const FIELDS: [&str; 10] = ["author", "body", "body_hash", "kind", "prev", "replaces", "seq", "sig", "ts", "v"];

fn get_str<'a>(m: &'a Map<String, Value>, field: &'static str) -> Result<&'a str, RecordError> {
    match m.get(field) {
        None => Err(RecordError::MissingField { field }),
        Some(Value::String(s)) => Ok(s),
        Some(_) => Err(RecordError::InvalidField { field, problem: "must be a string" }),
    }
}

fn get_uint(m: &Map<String, Value>, field: &'static str) -> Result<u64, RecordError> {
    match m.get(field) {
        None => Err(RecordError::MissingField { field }),
        Some(Value::Number(n)) => match n.as_u64() {
            Some(u) if u <= MAX_SAFE_INTEGER => Ok(u),
            _ => Err(RecordError::InvalidField { field, problem: "must be an integer from 0 to 2^53 - 1" }),
        },
        Some(_) => Err(RecordError::InvalidField { field, problem: "must be an integer" }),
    }
}

fn hex_field<T>(field: &'static str, r: Result<T, HexError>) -> Result<T, RecordError> {
    r.map_err(|e| RecordError::InvalidField {
        field,
        problem: match e {
            HexError::Length { .. } => "wrong number of hex characters",
            HexError::NotLowercaseHex => "must be lowercase hex",
        },
    })
}

impl Record {
    /// Parses and verifies one record from JSON text (one JSONL line).
    ///
    /// Applies every input rule of ARCHITECTURE 4.2: size limit before
    /// parsing, strict JSON (no duplicate keys, integer tokens only, no lone
    /// surrogates, depth ≤ 16), then [`Record::from_value`].
    pub fn from_json_str(text: &str) -> Result<Record, RecordError> {
        if text.len() > MAX_LINE_BYTES {
            return Err(RecordError::TooLarge { actual: text.len(), limit: MAX_LINE_BYTES });
        }
        let v = json::parse(text, MAX_DEPTH)?;
        Record::from_value(v)
    }

    /// Builds and verifies a record from an in-memory JSON value.
    ///
    /// A `serde_json::Value` cannot hold duplicate keys or lone surrogates,
    /// so for untrusted text use [`Record::from_json_str`], which rejects
    /// them. This function still rejects floats, out-of-range integers, and
    /// excessive depth.
    pub fn from_value(v: Value) -> Result<Record, RecordError> {
        let Value::Object(mut m) = v else {
            return Err(RecordError::NotObject);
        };
        for k in m.keys() {
            if !FIELDS.contains(&k.as_str()) {
                return Err(RecordError::UnknownField { field: k.clone() });
            }
        }
        let version = match m.get("v") {
            None => return Err(RecordError::MissingField { field: "v" }),
            Some(Value::Number(n)) => match n.as_i64() {
                Some(i) => i,
                None => return Err(RecordError::InvalidField { field: "v", problem: "must be the integer 1" }),
            },
            Some(_) => return Err(RecordError::InvalidField { field: "v", problem: "must be the integer 1" }),
        };
        if version != VERSION as i64 {
            return Err(RecordError::Version { version });
        }
        let author = hex_field("author", AuthorId::from_hex(get_str(&m, "author")?))?;
        let seq = get_uint(&m, "seq")?;
        let prev = match m.get("prev") {
            None => return Err(RecordError::MissingField { field: "prev" }),
            Some(Value::Null) => None,
            Some(Value::String(s)) => Some(hex_field("prev", RecordId::from_hex(s))?),
            Some(_) => return Err(RecordError::InvalidField { field: "prev", problem: "must be a string or null" }),
        };
        let ts = get_uint(&m, "ts")?;
        let kind = get_str(&m, "kind")?.to_owned();
        let replaces = match m.get("replaces") {
            None => None,
            Some(Value::String(s)) => Some(hex_field("replaces", RecordId::from_hex(s))?),
            Some(Value::Null) => {
                return Err(RecordError::InvalidField { field: "replaces", problem: "must be omitted when absent, not null" });
            }
            Some(_) => return Err(RecordError::InvalidField { field: "replaces", problem: "must be a string" }),
        };
        let body_hash = hex_field("body_hash", Hex32::from_hex(get_str(&m, "body_hash")?))?;
        let sig = hex_field("sig", Sig::from_hex(get_str(&m, "sig")?))?;
        let header = Header { v: VERSION, author, seq, prev, ts, kind, replaces, body_hash };
        header.check()?;
        let header_canonical = header.to_canonical();
        if header_canonical.len() > MAX_HEADER_BYTES {
            return Err(RecordError::HeaderTooLarge { actual: header_canonical.len(), limit: MAX_HEADER_BYTES });
        }
        let body = match m.remove("body") {
            None => None,
            Some(Value::Object(b)) => {
                let canonical = body_canonical(&b)?;
                if keys::sha256(canonical.as_bytes()) != body_hash.0 {
                    return Err(RecordError::BodyHashMismatch);
                }
                Some(Arc::new(BodyData { map: b, canonical }))
            }
            Some(_) => return Err(RecordError::BodyNotObject),
        };
        keys::verify_with_domain(&author, RECORD_DOMAIN, header_canonical.as_bytes(), &sig)
            .map_err(|detail| RecordError::Signature { detail })?;
        let id = RecordId(keys::sha256(header_canonical.as_bytes()));
        Ok(Record { inner: Arc::new(Inner { header, id, sig, header_canonical, body }) })
    }

    /// The record ID: SHA-256 of the canonical header.
    pub fn id(&self) -> RecordId {
        self.inner.id
    }
    /// The signed header.
    pub fn header(&self) -> &Header {
        &self.inner.header
    }
    /// The author.
    pub fn author(&self) -> AuthorId {
        self.inner.header.author
    }
    /// Position in the author's log.
    pub fn seq(&self) -> u64 {
        self.inner.header.seq
    }
    /// ID of the previous record in the author's log.
    pub fn prev(&self) -> Option<RecordId> {
        self.inner.header.prev
    }
    /// Time claimed by the author, Unix milliseconds.
    pub fn ts(&self) -> u64 {
        self.inner.header.ts
    }
    /// Record type.
    pub fn kind(&self) -> &str {
        &self.inner.header.kind
    }
    /// ID of the record this one supersedes.
    pub fn replaces(&self) -> Option<RecordId> {
        self.inner.header.replaces
    }
    /// SHA-256 of the canonical body.
    pub fn body_hash(&self) -> Hex32 {
        self.inner.header.body_hash
    }
    /// The signature.
    pub fn sig(&self) -> Sig {
        self.inner.sig
    }
    /// The body as received, or `None` if it was removed (redacted) or not
    /// yet received.
    pub fn body(&self) -> Option<&Map<String, Value>> {
        self.inner.body.as_ref().map(|b| &b.map)
    }
    /// True if the body is present.
    pub fn has_body(&self) -> bool {
        self.inner.body.is_some()
    }
    /// Canonical JSON of the header: the bytes that are hashed and signed
    /// (after the domain prefix).
    pub fn header_canonical(&self) -> &str {
        &self.inner.header_canonical
    }
    /// Canonical JSON of the body, if present.
    pub fn body_canonical(&self) -> Option<&str> {
        self.inner.body.as_ref().map(|b| b.canonical.as_str())
    }

    /// The same record without its body (header and signature only).
    pub fn without_body(&self) -> Record {
        if !self.has_body() {
            return self.clone();
        }
        let i = &self.inner;
        Record {
            inner: Arc::new(Inner {
                header: i.header.clone(),
                id: i.id,
                sig: i.sig,
                header_canonical: i.header_canonical.clone(),
                body: None,
            }),
        }
    }

    /// The same record with `body` attached. Fails with
    /// [`RecordError::BodyHashMismatch`] unless `sha256(canonical(body))`
    /// equals `body_hash`.
    pub fn with_body(&self, body: Map<String, Value>) -> Result<Record, RecordError> {
        let canonical = body_canonical(&body)?;
        if keys::sha256(canonical.as_bytes()) != self.inner.header.body_hash.0 {
            return Err(RecordError::BodyHashMismatch);
        }
        let i = &self.inner;
        Ok(Record {
            inner: Arc::new(Inner {
                header: i.header.clone(),
                id: i.id,
                sig: i.sig,
                header_canonical: i.header_canonical.clone(),
                body: Some(Arc::new(BodyData { map: body, canonical })),
            }),
        })
    }

    /// Takes the body from `other`, which must be the same record (same ID).
    /// Returns `self` unchanged if `other` has no body.
    pub fn with_body_from(&self, other: &Record) -> Result<Record, RecordError> {
        if other.id() != self.id() {
            return Err(RecordError::WrongRecord { this: self.id(), other: other.id() });
        }
        if !other.has_body() {
            return Ok(self.clone());
        }
        let i = &self.inner;
        Ok(Record {
            inner: Arc::new(Inner {
                header: i.header.clone(),
                id: i.id,
                sig: i.sig,
                header_canonical: i.header_canonical.clone(),
                body: other.inner.body.clone(),
            }),
        })
    }

    /// The wire form: `canonical_json(header + sig + body)`, with `body`
    /// omitted when absent. This is also one JSONL line (without the newline).
    pub fn to_json_string(&self) -> String {
        let h = &self.inner.header;
        let body = self.body_canonical();
        let mut s = String::with_capacity(self.inner.header_canonical.len() + 140 + body.map_or(0, str::len));
        // Keys in UTF-16 order: author, body, body_hash, kind, prev,
        // replaces, seq, sig, ts, v.
        s.push_str("{\"author\":\"");
        s.push_str(&h.author.to_hex());
        s.push('"');
        if let Some(b) = body {
            s.push_str(",\"body\":");
            s.push_str(b);
        }
        s.push_str(",\"body_hash\":\"");
        s.push_str(&h.body_hash.to_hex());
        s.push_str("\",\"kind\":");
        canonical::write_string(&h.kind, &mut s);
        s.push_str(",\"prev\":");
        push_opt_id(&mut s, h.prev);
        if let Some(r) = h.replaces {
            s.push_str(",\"replaces\":\"");
            s.push_str(&r.to_hex());
            s.push('"');
        }
        s.push_str(",\"seq\":");
        s.push_str(&h.seq.to_string());
        s.push_str(",\"sig\":\"");
        s.push_str(&self.inner.sig.to_hex());
        s.push_str("\",\"ts\":");
        s.push_str(&h.ts.to_string());
        s.push_str(",\"v\":");
        s.push_str(&h.v.to_string());
        s.push('}');
        s
    }

    /// The wire form as a JSON value.
    pub fn to_value(&self) -> Value {
        let h = &self.inner.header;
        let mut m = Map::new();
        m.insert("v".into(), Value::from(h.v));
        m.insert("author".into(), Value::String(h.author.to_hex()));
        m.insert("seq".into(), Value::from(h.seq));
        m.insert("prev".into(), h.prev.map_or(Value::Null, |p| Value::String(p.to_hex())));
        m.insert("ts".into(), Value::from(h.ts));
        m.insert("kind".into(), Value::String(h.kind.clone()));
        if let Some(r) = h.replaces {
            m.insert("replaces".into(), Value::String(r.to_hex()));
        }
        m.insert("body_hash".into(), Value::String(h.body_hash.to_hex()));
        m.insert("sig".into(), Value::String(self.inner.sig.to_hex()));
        if let Some(b) = self.body() {
            m.insert("body".into(), Value::Object(b.clone()));
        }
        Value::Object(m)
    }
}

impl Serialize for Record {
    /// Serializes the wire object. Fields are emitted in canonical order, but
    /// only [`Record::to_json_string`] guarantees canonical bytes.
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let h = &self.inner.header;
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("author", &h.author)?;
        if let Some(b) = self.body() {
            m.serialize_entry("body", b)?;
        }
        m.serialize_entry("body_hash", &h.body_hash)?;
        m.serialize_entry("kind", &h.kind)?;
        m.serialize_entry("prev", &h.prev)?;
        if let Some(r) = &h.replaces {
            m.serialize_entry("replaces", r)?;
        }
        m.serialize_entry("seq", &h.seq)?;
        m.serialize_entry("sig", &self.inner.sig)?;
        m.serialize_entry("ts", &h.ts)?;
        m.serialize_entry("v", &h.v)?;
        m.end()
    }
}

impl<'de> Deserialize<'de> for Record {
    /// Captures the record's raw JSON text and parses it with
    /// [`Record::from_json_str`], so every rule applies on this path too:
    /// duplicate keys, number forms, size and depth limits, and the
    /// signature. Works with `serde_json` deserializers (text or `Value`);
    /// a `Value` has already lost duplicate keys, so pass text where the
    /// input is untrusted.
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = Box::<serde_json::value::RawValue>::deserialize(d)?;
        Record::from_json_str(raw.get()).map_err(serde::de::Error::custom)
    }
}

/// Parses JSON Lines text. Yields `(line_number, result)` for each non-blank
/// line; line numbers start at 1. A trailing `\r` is ignored.
pub fn parse_jsonl(text: &str) -> impl Iterator<Item = (usize, Result<Record, RecordError>)> + '_ {
    text.split('\n').enumerate().filter_map(|(i, line)| {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.trim_matches([' ', '\t']).is_empty() {
            None
        } else {
            Some((i + 1, Record::from_json_str(line)))
        }
    })
}

/// Writes records as JSON Lines: one wire-form record per line, each ending
/// in `\n`.
pub fn to_jsonl<'a>(records: impl IntoIterator<Item = &'a Record>) -> String {
    let mut s = String::new();
    for r in records {
        s.push_str(&r.to_json_string());
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn kp() -> Keypair {
        Keypair::from_seed([42; 32])
    }

    fn body(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn first(kind: &str, b: Value) -> Record {
        sign_record(&kp(), Unsigned { seq: 1, prev: None, ts: 1000, kind: kind.into(), replaces: None, body: body(b) })
            .unwrap()
    }

    #[test]
    fn serde_deserialize_applies_strict_rules() {
        let r = first("profile", json!({"name": "Ada", "role": "seller"}));
        let wire = r.to_json_string();
        // Text path: same result as from_json_str.
        let back: Record = serde_json::from_str(&wire).unwrap();
        assert_eq!(back.id(), r.id());
        // Inside a container (as in an ingest array).
        let arr: Vec<Record> = serde_json::from_str(&format!("[{wire}]")).unwrap();
        assert_eq!(arr[0].id(), r.id());
        // Value path still verifies.
        let v: Record = serde_json::from_value(r.to_value()).unwrap();
        assert_eq!(v.id(), r.id());
        // A duplicate key is rejected on the serde path, as on the text path.
        let dup = wire.replacen("{\"author\"", "{\"kind\":\"profile\",\"author\"", 1);
        assert!(Record::from_json_str(&dup).is_err());
        assert!(serde_json::from_str::<Record>(&dup).is_err());
        assert!(serde_json::from_str::<Vec<Record>>(&format!("[{dup}]")).is_err());
        // Number forms that the strict parser rejects.
        assert!(serde_json::from_str::<Record>(&wire.replace("\"v\":1", "\"v\":1.0")).is_err());
    }

    #[test]
    fn sign_serialize_parse_verify() {
        let r = first("review", json!({"product": "gtin:00012345678905", "rating": 5, "text": "Gut. \"Sehr\"\n"}));
        let wire = r.to_json_string();
        assert!(canonical::is_canonical(&wire), "{wire}");
        assert_eq!(wire, canonical::to_canonical_json(&r.to_value()).unwrap());
        let back = Record::from_json_str(&wire).unwrap();
        assert_eq!(back.id(), r.id());
        assert_eq!(back.to_json_string(), wire);
        assert_eq!(back.body(), r.body());
        assert_eq!(back.header_canonical(), r.header_canonical());
        assert_eq!(r.header_canonical(), canonical::to_canonical_json(&serde_json::to_value(r.header()).unwrap()).unwrap());
        assert_eq!(r.id(), RecordId(keys::sha256(r.header_canonical().as_bytes())));
        assert_eq!(r.body_hash(), body_hash(r.body().unwrap()).unwrap());
        // serde round trip
        let via_serde: Record = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(via_serde.id(), r.id());
    }

    #[test]
    fn header_only_and_body_fill() {
        let r = first("profile", json!({"name": "A", "role": "buyer"}));
        let h = r.without_body();
        assert!(!h.has_body());
        let wire = h.to_json_string();
        assert!(!wire.contains("\"body\":"));
        let parsed = Record::from_json_str(&wire).unwrap();
        assert_eq!(parsed.id(), r.id());
        assert!(parsed.body().is_none());
        let filled = parsed.with_body(r.body().unwrap().clone()).unwrap();
        assert_eq!(filled.to_json_string(), r.to_json_string());
        assert_eq!(parsed.with_body(body(json!({"name": "B", "role": "buyer"}))), Err(RecordError::BodyHashMismatch));
        assert_eq!(parsed.with_body_from(&r).unwrap().to_json_string(), r.to_json_string());
        let other = first("profile", json!({"name": "C", "role": "buyer"}));
        assert!(matches!(parsed.with_body_from(&other), Err(RecordError::WrongRecord { .. })));
    }

    #[test]
    fn normalizes_noncanonical_input() {
        let r = first("flag", json!({"target": "ab".repeat(32), "reason": "spam"}));
        let pretty = serde_json::to_string_pretty(&r.to_value()).unwrap();
        let back = Record::from_json_str(&pretty).unwrap();
        assert_eq!(back.to_json_string(), r.to_json_string());
    }

    fn tamper(r: &Record, f: impl FnOnce(&mut Map<String, Value>)) -> Result<Record, RecordError> {
        let mut v = r.to_value();
        f(v.as_object_mut().unwrap());
        Record::from_json_str(&serde_json::to_string(&v).unwrap())
    }

    #[test]
    fn envelope_errors() {
        let r = first("profile", json!({"name": "A", "role": "buyer"}));
        assert_eq!(tamper(&r, |m| { m.insert("x".into(), json!(1)); }), Err(RecordError::UnknownField { field: "x".into() }));
        assert_eq!(tamper(&r, |m| { m.remove("sig"); }), Err(RecordError::MissingField { field: "sig" }));
        assert_eq!(tamper(&r, |m| { m.remove("prev"); }), Err(RecordError::MissingField { field: "prev" }));
        assert_eq!(tamper(&r, |m| { m.insert("v".into(), json!(2)); }), Err(RecordError::Version { version: 2 }));
        assert!(matches!(tamper(&r, |m| { m.insert("v".into(), json!("1")); }), Err(RecordError::InvalidField { field: "v", .. })));
        assert!(matches!(tamper(&r, |m| { m.insert("seq".into(), json!(0)); }), Err(RecordError::InvalidField { field: "seq", .. })));
        assert!(matches!(tamper(&r, |m| { m.insert("seq".into(), json!(-1)); }), Err(RecordError::InvalidField { field: "seq", .. })));
        assert_eq!(tamper(&r, |m| { m.insert("seq".into(), json!(2)); }), Err(RecordError::PrevMismatch));
        assert!(matches!(tamper(&r, |m| { m.insert("prev".into(), json!(1)); }), Err(RecordError::InvalidField { field: "prev", .. })));
        assert_eq!(tamper(&r, |m| { m.insert("prev".into(), json!("00".repeat(32))); }), Err(RecordError::PrevMismatch));
        assert!(matches!(tamper(&r, |m| { m.insert("ts".into(), json!(-5)); }), Err(RecordError::InvalidField { field: "ts", .. })));
        assert!(matches!(tamper(&r, |m| { m.insert("ts".into(), json!(true)); }), Err(RecordError::InvalidField { field: "ts", .. })));
        assert_eq!(tamper(&r, |m| { m.insert("kind".into(), json!("Profile")); }), Err(RecordError::BadKind { kind: "Profile".into() }));
        assert!(matches!(tamper(&r, |m| { m.insert("replaces".into(), json!(null)); }), Err(RecordError::InvalidField { field: "replaces", .. })));
        assert!(matches!(tamper(&r, |m| { m.insert("author".into(), json!(r.author().to_hex().to_uppercase())); }), Err(RecordError::InvalidField { field: "author", .. })));
        assert!(matches!(tamper(&r, |m| { m.insert("sig".into(), json!("00")); }), Err(RecordError::InvalidField { field: "sig", .. })));
        assert!(matches!(tamper(&r, |m| { m.insert("body_hash".into(), json!(5)); }), Err(RecordError::InvalidField { field: "body_hash", .. })));
        assert_eq!(tamper(&r, |m| { m.insert("body".into(), json!([])); }), Err(RecordError::BodyNotObject));
        assert_eq!(tamper(&r, |m| { m.insert("body".into(), json!({"name": "B", "role": "buyer"})); }), Err(RecordError::BodyHashMismatch));
        assert!(matches!(tamper(&r, |m| { m.insert("ts".into(), json!(1001)); }), Err(RecordError::Signature { detail: SignatureError::Invalid })));
        assert!(matches!(tamper(&r, |m| { m.insert("author".into(), json!(Keypair::from_seed([1; 32]).author().to_hex())); }), Err(RecordError::Signature { .. })));
        assert_eq!(Record::from_json_str("[]"), Err(RecordError::NotObject));
        assert!(matches!(Record::from_json_str("{"), Err(RecordError::Json { .. })));
    }

    #[test]
    fn strict_parse_rules_apply_to_records() {
        let r = first("profile", json!({"name": "A", "role": "buyer"}));
        let w = r.to_json_string();
        let dup = w.replacen("{\"author\"", "{\"kind\":\"profile\",\"author\"", 1);
        assert!(matches!(Record::from_json_str(&dup), Err(RecordError::Json { detail: JsonError::DuplicateKey { .. } })));
        let float = w.replace("\"v\":1", "\"v\":1.0");
        assert!(matches!(Record::from_json_str(&float), Err(RecordError::Json { detail: JsonError::NotInteger { .. } })));
        let big = format!("{}{}", " ".repeat(MAX_LINE_BYTES), w);
        assert!(matches!(Record::from_json_str(&big), Err(RecordError::TooLarge { .. })));
    }

    #[test]
    fn size_and_depth_limits() {
        let k = kp();
        let mk = |b: Value| sign_record(&k, Unsigned { seq: 1, prev: None, ts: 0, kind: "x".into(), replaces: None, body: body(b) });
        // Body exactly at the limit: {"t":"..."} = 8 bytes of overhead.
        let at = "a".repeat(MAX_BODY_BYTES - 8);
        assert!(mk(json!({"t": at})).is_ok());
        let over = "a".repeat(MAX_BODY_BYTES - 7);
        assert!(matches!(mk(json!({"t": over})), Err(RecordError::BodyTooLarge { .. })));
        // Depth: record 1, body 2, so 14 more levels are allowed inside the body.
        let mut v = json!(1);
        for _ in 0..14 {
            v = json!([v]);
        }
        assert!(mk(json!({"d": v.clone()})).is_ok());
        let deeper = json!({"d": [v]});
        assert_eq!(mk(deeper), Err(RecordError::TooDeep { max: MAX_DEPTH }));
        assert!(matches!(mk(json!({"f": 1.5})), Err(RecordError::Canonical { .. })));
        // Header too large via a long kind is impossible (kind ≤ 32), so the
        // header limit cannot be reached with valid fields.
        let long_kind = sign_record(&k, Unsigned { seq: 1, prev: None, ts: 0, kind: "a".repeat(33), replaces: None, body: Map::new() });
        assert!(matches!(long_kind, Err(RecordError::BadKind { .. })));
    }

    #[test]
    fn deep_record_from_text_rejected_at_depth_17() {
        let r = first("x", json!({}));
        let mut b = json!(1);
        for _ in 0..15 {
            b = json!([b]);
        }
        // Body {"d": 15 arrays} has depth 16 under the record → 17 total.
        let mut v = r.to_value();
        v["body"] = json!({ "d": b });
        let text = serde_json::to_string(&v).unwrap();
        assert_eq!(Record::from_json_str(&text), Err(RecordError::TooDeep { max: 16 }));
    }

    #[test]
    fn kind_format() {
        for k in ["a", "attest.identity", "x_1.y", &"a".repeat(32)] {
            assert!(is_valid_kind(k), "{k}");
        }
        for k in ["", "A", "1a", ".a", "_a", "a-b", "a b", "é", &"a".repeat(33)] {
            assert!(!is_valid_kind(k), "{k}");
        }
    }

    #[test]
    fn jsonl_roundtrip() {
        let a = first("x", json!({"n": 1}));
        let b = first("y", json!({"n": 2}));
        let text = to_jsonl([&a, &b]);
        let mut lines = format!("{text}\n  \r\nnot json\r\n");
        lines.push_str(&b.to_json_string());
        lines.push_str("\r\n");
        let got: Vec<_> = parse_jsonl(&lines).collect();
        assert_eq!(got.len(), 4);
        assert_eq!(got[0].0, 1);
        assert_eq!(got[0].1.as_ref().unwrap().id(), a.id());
        assert_eq!(got[1].1.as_ref().unwrap().id(), b.id());
        assert_eq!(got[2].0, 5);
        assert!(got[2].1.is_err());
        assert_eq!(got[3].1.as_ref().unwrap().id(), b.id());
    }

    #[test]
    fn errors_serialize() {
        let e = RecordError::MissingField { field: "sig" };
        assert_eq!(serde_json::to_value(&e).unwrap(), json!({"error": "missing_field", "field": "sig"}));
        let e = RecordError::Signature { detail: SignatureError::Invalid };
        assert_eq!(serde_json::to_value(&e).unwrap(), json!({"error": "signature", "detail": "invalid"}));
    }
}
