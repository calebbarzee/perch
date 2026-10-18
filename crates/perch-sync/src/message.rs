//! Protocol messages: typed form, strict parsing, and JSON encoding.
//!
//! Records travel as JSON objects inside messages. This module never decodes
//! them: it hands their exact text to the store, which parses and verifies
//! them with `perch-core`.

use serde::Deserialize;
use serde_json::value::RawValue;
use std::fmt::Write as _;

/// Value of `hello.protocol`.
pub const PROTOCOL: &str = "perch-sync";
/// Protocol version implemented by this crate.
pub const VERSION: u32 = 1;

/// Hard protocol limits (docs/sync-protocol.md section 9.1).
pub mod limits {
    /// Maximum size of one message, in bytes.
    pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
    /// Maximum records in `records`, `push`, or `bodies`.
    pub const MAX_RECORDS: usize = 500;
    /// Maximum entries in one `heads` page.
    pub const MAX_HEADS: usize = 4096;
    /// Maximum entries in one `want`.
    pub const MAX_WANTS: usize = 1024;
    /// Maximum IDs in one `want_bodies`.
    pub const MAX_BODY_IDS: usize = 1024;
    /// Maximum authors in one `want_evidence`.
    pub const MAX_EVIDENCE_AUTHORS: usize = 256;
    /// Maximum pairs in one `evidence` or `push`.
    pub const MAX_PAIRS: usize = 256;
    /// Maximum addresses in one `peers`.
    pub const MAX_PEERS: usize = 100;
    /// Maximum length of one peer address.
    pub const MAX_ADDRESS_LEN: usize = 256;
    /// Smallest `max_message_bytes` a peer may announce: two maximal record lines.
    pub const MIN_MESSAGE_BYTES: usize = 2 * 131_072 + 1024;
    /// Maximum length of `hello.software` and `error.message`.
    pub const MAX_TEXT_LEN: usize = 256;
    /// Largest integer allowed in JSON (2^53 - 1).
    pub const MAX_SAFE_INT: u64 = (1 << 53) - 1;
}

/// Whether a peer can accept inbound connections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Node,
    Browser,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Node => "node",
            Role::Browser => "browser",
        }
    }
}

/// Receive limits a peer announces in `hello`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_message_bytes: usize,
    pub max_records: usize,
    pub max_heads: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_message_bytes: limits::MAX_MESSAGE_BYTES,
            max_records: limits::MAX_RECORDS,
            max_heads: limits::MAX_HEADS,
        }
    }
}

impl Limits {
    /// Element-wise minimum, also capped at the protocol limits.
    pub fn min(self, other: Limits) -> Limits {
        Limits {
            max_message_bytes: self
                .max_message_bytes
                .min(other.max_message_bytes)
                .min(limits::MAX_MESSAGE_BYTES),
            max_records: self.max_records.min(other.max_records).min(limits::MAX_RECORDS),
            max_heads: self.max_heads.min(other.max_heads).min(limits::MAX_HEADS),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hello {
    pub versions: Vec<u32>,
    pub node: String,
    pub role: Role,
    pub software: String,
    pub listen: Option<String>,
    pub limits: Limits,
}

/// One entry of a `heads` page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadEntry {
    pub author: String,
    pub seq: u64,
    pub id: Option<String>,
    pub forked: bool,
    /// IDs of the sender's evidence pair, sorted by (seq, id). A hint only.
    pub evidence: Option<[String; 2]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct WantEntry {
    pub author: String,
    pub from_seq: u64,
    /// Last seq the requester can store. `None` means no limit.
    #[serde(default)]
    pub to_seq: Option<u64>,
}

/// Error codes of the `error` message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    UnsupportedVersion,
    BadMessage,
    TooLarge,
    RateLimited,
    Misbehavior,
    SelfConnection,
    DuplicateConnection,
    ShuttingDown,
    /// A code this version does not know.
    Other(String),
}

impl ErrorCode {
    pub fn as_str(&self) -> &str {
        match self {
            ErrorCode::UnsupportedVersion => "unsupported_version",
            ErrorCode::BadMessage => "bad_message",
            ErrorCode::TooLarge => "too_large",
            ErrorCode::RateLimited => "rate_limited",
            ErrorCode::Misbehavior => "misbehavior",
            ErrorCode::SelfConnection => "self_connection",
            ErrorCode::DuplicateConnection => "duplicate_connection",
            ErrorCode::ShuttingDown => "shutting_down",
            ErrorCode::Other(s) => s,
        }
    }

    pub fn parse(s: &str) -> ErrorCode {
        match s {
            "unsupported_version" => ErrorCode::UnsupportedVersion,
            "bad_message" => ErrorCode::BadMessage,
            "too_large" => ErrorCode::TooLarge,
            "rate_limited" => ErrorCode::RateLimited,
            "misbehavior" => ErrorCode::Misbehavior,
            "self_connection" => ErrorCode::SelfConnection,
            "duplicate_connection" => ErrorCode::DuplicateConnection,
            "shutting_down" => ErrorCode::ShuttingDown,
            other => ErrorCode::Other(other.to_string()),
        }
    }

    /// Whether this code always closes the session.
    pub fn is_fatal(&self) -> bool {
        !matches!(self, ErrorCode::RateLimited | ErrorCode::Other(_))
    }
}

/// A protocol message. Records are kept as their exact JSON text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Hello(Hello),
    GetHeads { after: Option<String>, limit: usize },
    Heads { heads: Vec<HeadEntry>, next: Option<String> },
    Want { wants: Vec<WantEntry> },
    Records { records: Vec<String>, truncated: bool },
    Push { records: Vec<String>, evidence: Vec<[String; 2]> },
    WantEvidence { authors: Vec<String> },
    Evidence { pairs: Vec<[String; 2]> },
    WantBodies { ids: Vec<String> },
    Bodies { records: Vec<String>, truncated: bool },
    GetPeers,
    Peers { peers: Vec<String> },
    Error { code: ErrorCode, message: String, fatal: bool },
    /// A message type this version does not know. Ignored (score 1).
    Unknown { type_name: String },
}

/// Why a message could not be parsed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("bad message: {0}")]
    Bad(String),
    #[error("message too large: {0}")]
    TooLarge(String),
}

impl ParseError {
    pub fn code(&self) -> ErrorCode {
        match self {
            ParseError::Bad(_) => ErrorCode::BadMessage,
            ParseError::TooLarge(_) => ErrorCode::TooLarge,
        }
    }
}

fn bad(msg: impl Into<String>) -> ParseError {
    ParseError::Bad(msg.into())
}

/// True for 64 lowercase hex characters.
pub fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn check_hex64(s: &str, what: &str) -> Result<(), ParseError> {
    if is_hex64(s) {
        Ok(())
    } else {
        Err(bad(format!("{what} is not 64 lowercase hex characters")))
    }
}

fn check_count(n: usize, max: usize, what: &str) -> Result<(), ParseError> {
    if n > max {
        Err(ParseError::TooLarge(format!("{n} {what}, limit {max}")))
    } else {
        Ok(())
    }
}

fn check_seq(n: u64, what: &str) -> Result<(), ParseError> {
    if n > limits::MAX_SAFE_INT {
        Err(bad(format!("{what} above 2^53 - 1")))
    } else {
        Ok(())
    }
}

/// True when `s` is an acceptable peer address.
pub fn is_valid_address(s: &str) -> bool {
    s.len() <= limits::MAX_ADDRESS_LEN
        && (s.starts_with("ws://") || s.starts_with("wss://"))
        && s.len() > "wss://".len()
        && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

// ---- raw (serde) shapes ----

#[derive(Deserialize)]
struct TypeOnly {
    #[serde(rename = "type")]
    ty: String,
}

#[derive(Deserialize)]
struct RawLimits {
    max_message_bytes: Option<u64>,
    max_records: Option<u64>,
    max_heads: Option<u64>,
}

#[derive(Deserialize)]
struct RawHello {
    protocol: String,
    versions: Vec<u64>,
    node: String,
    role: String,
    #[serde(default)]
    software: String,
    #[serde(default)]
    listen: Option<String>,
    #[serde(default)]
    limits: Option<RawLimits>,
}

#[derive(Deserialize)]
struct RawGetHeads {
    #[serde(default)]
    after: Option<String>,
    #[serde(default)]
    limit: Option<u64>,
}

#[derive(Deserialize)]
struct RawHead {
    author: String,
    seq: u64,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    forked: bool,
    #[serde(default)]
    evidence: Option<[String; 2]>,
}

#[derive(Deserialize)]
struct RawHeads {
    heads: Vec<RawHead>,
    #[serde(default)]
    next: Option<String>,
}

#[derive(Deserialize)]
struct RawWant {
    wants: Vec<WantEntry>,
}

#[derive(Deserialize)]
struct RawRecords {
    records: Vec<Box<RawValue>>,
    #[serde(default)]
    truncated: bool,
}

#[derive(Deserialize)]
struct RawPush {
    #[serde(default)]
    records: Vec<Box<RawValue>>,
    #[serde(default)]
    evidence: Vec<[Box<RawValue>; 2]>,
}

#[derive(Deserialize)]
struct RawWantEvidence {
    authors: Vec<String>,
}

#[derive(Deserialize)]
struct RawEvidence {
    pairs: Vec<[Box<RawValue>; 2]>,
}

#[derive(Deserialize)]
struct RawWantBodies {
    ids: Vec<String>,
}

#[derive(Deserialize)]
struct RawBodies {
    records: Vec<Box<RawValue>>,
    #[serde(default)]
    truncated: bool,
}

#[derive(Deserialize)]
struct RawPeers {
    peers: Vec<String>,
}

#[derive(Deserialize)]
struct RawError {
    code: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    fatal: Option<bool>,
}

fn de<'a, T: Deserialize<'a>>(text: &'a str) -> Result<T, ParseError> {
    serde_json::from_str(text).map_err(|e| bad(e.to_string()))
}

fn raw_records(v: Vec<Box<RawValue>>) -> Result<Vec<String>, ParseError> {
    v.into_iter()
        .map(|r| {
            let s = r.get();
            if s.starts_with('{') {
                Ok(s.to_string())
            } else {
                Err(bad("record is not a JSON object"))
            }
        })
        .collect()
}

fn raw_pairs(v: Vec<[Box<RawValue>; 2]>) -> Result<Vec<[String; 2]>, ParseError> {
    v.into_iter()
        .map(|[a, b]| {
            let mut out = raw_records(vec![a, b])?;
            let b = out.pop().unwrap_or_default();
            let a = out.pop().unwrap_or_default();
            Ok([a, b])
        })
        .collect()
}

fn clamp_usize(v: Option<u64>, default: usize) -> usize {
    match v {
        Some(n) => usize::try_from(n).unwrap_or(usize::MAX).min(default),
        None => default,
    }
}

impl Message {
    /// Parses one message. Size limits on the text itself are checked by the caller.
    pub fn parse(text: &str) -> Result<Message, ParseError> {
        let t: TypeOnly = de(text)?;
        use limits::*;
        let msg = match t.ty.as_str() {
            "hello" => {
                let r: RawHello = de(text)?;
                if r.protocol != PROTOCOL {
                    return Err(bad(format!("unknown protocol {:?}", r.protocol)));
                }
                check_count(r.versions.len(), 16, "versions")?;
                let role = match r.role.as_str() {
                    "node" => Role::Node,
                    "browser" => Role::Browser,
                    other => return Err(bad(format!("unknown role {other:?}"))),
                };
                if r.node.len() != 32 || !r.node.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(bad("hello.node must be 32 hex characters"));
                }
                if r.software.chars().count() > MAX_TEXT_LEN {
                    return Err(bad("hello.software too long"));
                }
                if let Some(l) = &r.listen
                    && !is_valid_address(l)
                {
                    return Err(bad("hello.listen is not a ws:// or wss:// URL"));
                }
                let d = Limits::default();
                let limits = match r.limits {
                    Some(l) => Limits {
                        max_message_bytes: clamp_usize(l.max_message_bytes, d.max_message_bytes),
                        max_records: clamp_usize(l.max_records, d.max_records),
                        max_heads: clamp_usize(l.max_heads, d.max_heads),
                    },
                    None => d,
                };
                if limits.max_records == 0
                    || limits.max_heads == 0
                    || limits.max_message_bytes < limits::MIN_MESSAGE_BYTES
                {
                    return Err(bad("hello.limits too small"));
                }
                Message::Hello(Hello {
                    versions: r
                        .versions
                        .into_iter()
                        .map(|v| u32::try_from(v).unwrap_or(u32::MAX))
                        .collect(),
                    node: r.node.to_ascii_lowercase(),
                    role,
                    software: r.software,
                    listen: r.listen,
                    limits,
                })
            }
            "get_heads" => {
                let r: RawGetHeads = de(text)?;
                if let Some(a) = &r.after {
                    check_hex64(a, "get_heads.after")?;
                }
                Message::GetHeads {
                    after: r.after,
                    limit: clamp_usize(r.limit, MAX_HEADS).max(1),
                }
            }
            "heads" => {
                let r: RawHeads = de(text)?;
                check_count(r.heads.len(), MAX_HEADS, "heads")?;
                let mut heads = Vec::with_capacity(r.heads.len());
                let mut last: Option<String> = None;
                for h in r.heads {
                    check_hex64(&h.author, "heads.author")?;
                    check_seq(h.seq, "heads.seq")?;
                    if let Some(prev) = &last
                        && h.author.as_str() <= prev.as_str()
                    {
                        return Err(bad("heads not sorted by author"));
                    }
                    match (&h.id, h.seq) {
                        (None, 0) => {}
                        (Some(id), s) if s > 0 => check_hex64(id, "heads.id")?,
                        _ => return Err(bad("heads.id must be null exactly when seq is 0")),
                    }
                    if let Some([a, b]) = &h.evidence {
                        check_hex64(a, "heads.evidence")?;
                        check_hex64(b, "heads.evidence")?;
                    }
                    last = Some(h.author.clone());
                    heads.push(HeadEntry {
                        author: h.author,
                        seq: h.seq,
                        id: h.id,
                        forked: h.forked || h.evidence.is_some(),
                        evidence: h.evidence,
                    });
                }
                if let Some(n) = &r.next {
                    check_hex64(n, "heads.next")?;
                }
                Message::Heads { heads, next: r.next }
            }
            "want" => {
                let r: RawWant = de(text)?;
                check_count(r.wants.len(), MAX_WANTS, "wants")?;
                for w in &r.wants {
                    check_hex64(&w.author, "want.author")?;
                    check_seq(w.from_seq, "want.from_seq")?;
                    if let Some(t) = w.to_seq {
                        check_seq(t, "want.to_seq")?;
                    }
                    if w.from_seq == 0 {
                        return Err(bad("want.from_seq must be at least 1"));
                    }
                }
                Message::Want { wants: r.wants }
            }
            "records" => {
                let r: RawRecords = de(text)?;
                check_count(r.records.len(), MAX_RECORDS, "records")?;
                Message::Records {
                    records: raw_records(r.records)?,
                    truncated: r.truncated,
                }
            }
            "push" => {
                let r: RawPush = de(text)?;
                check_count(r.records.len(), MAX_RECORDS, "records")?;
                check_count(r.evidence.len(), MAX_PAIRS, "evidence pairs")?;
                Message::Push {
                    records: raw_records(r.records)?,
                    evidence: raw_pairs(r.evidence)?,
                }
            }
            "want_evidence" => {
                let r: RawWantEvidence = de(text)?;
                check_count(r.authors.len(), MAX_EVIDENCE_AUTHORS, "authors")?;
                for a in &r.authors {
                    check_hex64(a, "want_evidence.authors")?;
                }
                Message::WantEvidence { authors: r.authors }
            }
            "evidence" => {
                let r: RawEvidence = de(text)?;
                check_count(r.pairs.len(), MAX_PAIRS, "evidence pairs")?;
                Message::Evidence {
                    pairs: raw_pairs(r.pairs)?,
                }
            }
            "want_bodies" => {
                let r: RawWantBodies = de(text)?;
                check_count(r.ids.len(), MAX_BODY_IDS, "ids")?;
                for id in &r.ids {
                    check_hex64(id, "want_bodies.ids")?;
                }
                Message::WantBodies { ids: r.ids }
            }
            "bodies" => {
                let r: RawBodies = de(text)?;
                check_count(r.records.len(), MAX_RECORDS, "records")?;
                Message::Bodies {
                    records: raw_records(r.records)?,
                    truncated: r.truncated,
                }
            }
            "get_peers" => Message::GetPeers,
            "peers" => {
                let r: RawPeers = de(text)?;
                check_count(r.peers.len(), MAX_PEERS, "peers")?;
                Message::Peers { peers: r.peers }
            }
            "error" => {
                let r: RawError = de(text)?;
                let code = ErrorCode::parse(&r.code);
                let fatal = r.fatal.unwrap_or_else(|| code.is_fatal());
                let message: String = r.message.chars().take(MAX_TEXT_LEN).collect();
                Message::Error { code, message, fatal }
            }
            other => Message::Unknown {
                type_name: other.chars().take(64).collect(),
            },
        };
        Ok(msg)
    }

    /// The `type` field of this message.
    pub fn type_name(&self) -> &str {
        match self {
            Message::Hello(_) => "hello",
            Message::GetHeads { .. } => "get_heads",
            Message::Heads { .. } => "heads",
            Message::Want { .. } => "want",
            Message::Records { .. } => "records",
            Message::Push { .. } => "push",
            Message::WantEvidence { .. } => "want_evidence",
            Message::Evidence { .. } => "evidence",
            Message::WantBodies { .. } => "want_bodies",
            Message::Bodies { .. } => "bodies",
            Message::GetPeers => "get_peers",
            Message::Peers { .. } => "peers",
            Message::Error { .. } => "error",
            Message::Unknown { type_name } => type_name,
        }
    }

    /// Number of records carried, counting both records of each pair.
    pub fn record_count(&self) -> usize {
        match self {
            Message::Records { records, .. } | Message::Bodies { records, .. } => records.len(),
            Message::Push { records, evidence } => records.len() + 2 * evidence.len(),
            Message::Evidence { pairs } => 2 * pairs.len(),
            _ => 0,
        }
    }

    /// Encodes the message as one line of JSON. Records are inserted verbatim.
    pub fn to_json(&self) -> String {
        let mut w = String::with_capacity(128);
        w.push_str("{\"type\":");
        push_str_json(&mut w, self.type_name());
        match self {
            Message::Hello(h) => {
                w.push_str(",\"protocol\":");
                push_str_json(&mut w, PROTOCOL);
                w.push_str(",\"versions\":[");
                for (i, v) in h.versions.iter().enumerate() {
                    if i > 0 {
                        w.push(',');
                    }
                    let _ = write!(w, "{v}");
                }
                w.push_str("],\"node\":");
                push_str_json(&mut w, &h.node);
                w.push_str(",\"role\":");
                push_str_json(&mut w, h.role.as_str());
                w.push_str(",\"software\":");
                push_str_json(&mut w, &h.software);
                w.push_str(",\"listen\":");
                push_opt_str_json(&mut w, h.listen.as_deref());
                let _ = write!(
                    w,
                    ",\"limits\":{{\"max_message_bytes\":{},\"max_records\":{},\"max_heads\":{}}}",
                    h.limits.max_message_bytes, h.limits.max_records, h.limits.max_heads
                );
            }
            Message::GetHeads { after, limit } => {
                w.push_str(",\"after\":");
                push_opt_str_json(&mut w, after.as_deref());
                let _ = write!(w, ",\"limit\":{limit}");
            }
            Message::Heads { heads, next } => {
                w.push_str(",\"heads\":[");
                for (i, h) in heads.iter().enumerate() {
                    if i > 0 {
                        w.push(',');
                    }
                    w.push_str("{\"author\":");
                    push_str_json(&mut w, &h.author);
                    let _ = write!(w, ",\"seq\":{},\"id\":", h.seq);
                    push_opt_str_json(&mut w, h.id.as_deref());
                    if h.forked {
                        w.push_str(",\"forked\":true");
                    }
                    if let Some([a, b]) = &h.evidence {
                        w.push_str(",\"evidence\":[");
                        push_str_json(&mut w, a);
                        w.push(',');
                        push_str_json(&mut w, b);
                        w.push(']');
                    }
                    w.push('}');
                }
                w.push_str("],\"next\":");
                push_opt_str_json(&mut w, next.as_deref());
            }
            Message::Want { wants } => {
                w.push_str(",\"wants\":[");
                for (i, e) in wants.iter().enumerate() {
                    if i > 0 {
                        w.push(',');
                    }
                    w.push_str("{\"author\":");
                    push_str_json(&mut w, &e.author);
                    let _ = write!(w, ",\"from_seq\":{}", e.from_seq);
                    if let Some(t) = e.to_seq {
                        let _ = write!(w, ",\"to_seq\":{t}");
                    }
                    w.push('}');
                }
                w.push(']');
            }
            Message::Records { records, truncated } => {
                w.push_str(",\"records\":");
                push_raw_list(&mut w, records);
                let _ = write!(w, ",\"truncated\":{truncated}");
            }
            Message::Push { records, evidence } => {
                w.push_str(",\"records\":");
                push_raw_list(&mut w, records);
                if !evidence.is_empty() {
                    w.push_str(",\"evidence\":");
                    push_pairs(&mut w, evidence);
                }
            }
            Message::WantEvidence { authors } => {
                w.push_str(",\"authors\":");
                push_str_list(&mut w, authors);
            }
            Message::Evidence { pairs } => {
                w.push_str(",\"pairs\":");
                push_pairs(&mut w, pairs);
            }
            Message::WantBodies { ids } => {
                w.push_str(",\"ids\":");
                push_str_list(&mut w, ids);
            }
            Message::Bodies { records, truncated } => {
                w.push_str(",\"records\":");
                push_raw_list(&mut w, records);
                let _ = write!(w, ",\"truncated\":{truncated}");
            }
            Message::GetPeers => {}
            Message::Peers { peers } => {
                w.push_str(",\"peers\":");
                push_str_list(&mut w, peers);
            }
            Message::Error { code, message, fatal } => {
                w.push_str(",\"code\":");
                push_str_json(&mut w, code.as_str());
                w.push_str(",\"message\":");
                push_str_json(&mut w, message);
                let _ = write!(w, ",\"fatal\":{fatal}");
            }
            Message::Unknown { .. } => {}
        }
        w.push('}');
        w
    }

    /// Builds an `error` message.
    pub fn error(code: ErrorCode, message: impl Into<String>) -> Message {
        let fatal = code.is_fatal();
        Message::Error {
            code,
            message: message.into(),
            fatal,
        }
    }
}

fn push_str_json(w: &mut String, s: &str) {
    // serde_json escaping is the reference; it cannot fail for &str.
    w.push_str(&serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into()));
}

fn push_opt_str_json(w: &mut String, s: Option<&str>) {
    match s {
        Some(s) => push_str_json(w, s),
        None => w.push_str("null"),
    }
}

fn push_str_list(w: &mut String, list: &[String]) {
    w.push('[');
    for (i, s) in list.iter().enumerate() {
        if i > 0 {
            w.push(',');
        }
        push_str_json(w, s);
    }
    w.push(']');
}

fn push_raw_list(w: &mut String, list: &[String]) {
    w.push('[');
    for (i, s) in list.iter().enumerate() {
        if i > 0 {
            w.push(',');
        }
        w.push_str(s);
    }
    w.push(']');
}

fn push_pairs(w: &mut String, pairs: &[[String; 2]]) {
    w.push('[');
    for (i, [a, b]) in pairs.iter().enumerate() {
        if i > 0 {
            w.push(',');
        }
        w.push('[');
        w.push_str(a);
        w.push(',');
        w.push_str(b);
        w.push(']');
    }
    w.push(']');
}





