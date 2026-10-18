//! One sync session per connection (docs/sync-protocol.md sections 4–10).
//!
//! A [`Session`] performs no I/O. The driver feeds it received text with
//! [`Session::on_message`], store changes with [`Session::on_changes`], and
//! sends the returned messages in order.

use std::collections::{BTreeMap, BTreeSet};

use perch_core::{AuthorId, EvidencePair, Record, RecordId};
use perch_store::{Change, Head, Insert, Reject, Store};

use crate::message::{ErrorCode, HeadEntry, Hello, Limits, Message, ParseError, Role, VERSION, WantEntry, limits};

/// Static settings of the local peer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionConfig {
    /// 32 hex characters, random per process start.
    pub node: String,
    pub role: Role,
    pub software: String,
    pub listen: Option<String>,
    pub limits: Limits,
    /// Heads entries one exchange may add beyond the local author count.
    /// A peer that sends more is closed with `misbehavior`; this bounds the
    /// memory a peer can make a session hold.
    pub max_extra_authors: usize,
}

/// Default for [`SessionConfig::max_extra_authors`].
pub const MAX_EXTRA_AUTHORS: usize = 100_000;

impl SessionConfig {
    pub fn new(node: impl Into<String>, role: Role) -> Self {
        SessionConfig {
            node: node.into(),
            role,
            software: format!("perch-sync/{}", env!("CARGO_PKG_VERSION")),
            listen: None,
            limits: Limits::default(),
            max_extra_authors: MAX_EXTRA_AUTHORS,
        }
    }

    fn hello(&self) -> Hello {
        Hello {
            versions: vec![VERSION],
            node: self.node.clone(),
            role: self.role,
            software: self.software.clone(),
            listen: self.listen.clone(),
            limits: self.limits,
        }
    }
}

/// Session state (docs/sync-protocol.md section 4.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    AwaitHello,
    Syncing,
    Live,
    Closed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::AwaitHello => "await_hello",
            State::Syncing => "syncing",
            State::Live => "live",
            State::Closed => "closed",
        }
    }
}

/// A fatal session error. Send [`SyncError::to_message`], then close.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{}: {detail}", code.as_str())]
pub struct SyncError {
    pub code: ErrorCode,
    pub detail: String,
}

impl SyncError {
    fn new(code: ErrorCode, detail: impl Into<String>) -> Self {
        SyncError { code, detail: detail.into() }
    }

    pub fn to_message(&self) -> Message {
        Message::error(self.code.clone(), self.detail.clone())
    }
}

/// Counters per session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub records_sent: u64,
    pub records_received: u64,
    /// Records received that changed the local store.
    pub records_accepted: u64,
}

/// Peer score points (docs/sync-protocol.md section 10).
pub mod score {
    pub const BAD_MESSAGE: u32 = 100;
    pub const INVALID_RECORD: u32 = 50;
    pub const LOG_REJECTED: u32 = 10;
    pub const FALSE_FORK_HINT: u32 = 20;
    pub const UNSOLICITED_RESPONSE: u32 = 10;
    pub const UNKNOWN_TYPE: u32 = 1;
    pub const LIMIT: u32 = 100;
}

/// Maximum IDs remembered as "peer lacks this body".
pub const MAX_LACKS_BODY: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WantPlan {
    from: u64,
    to: Option<u64>,
}

#[derive(Clone, Debug)]
struct InflightWant {
    author: AuthorId,
    head_before: u64,
    fork_before: Option<u64>,
}

/// Messages being built during one call.
#[derive(Default)]
struct Out {
    msgs: Vec<Message>,
    push_records: Vec<String>,
    push_pairs: Vec<[String; 2]>,
}

fn pair_wire(p: &EvidencePair) -> [String; 2] {
    [p.a().without_body().to_json_string(), p.b().without_body().to_json_string()]
}

fn author_of(s: &str) -> Option<AuthorId> {
    AuthorId::from_hex(s).ok()
}

/// The protocol state for one connection.
#[derive(Clone, Debug)]
pub struct Session {
    pub id: u64,
    cfg: SessionConfig,
    outbound: bool,
    state: State,
    peer: Option<Hello>,
    send: Limits,
    heads_inflight: bool,
    heads_rounds: u64,
    /// `after` of the `get_heads` in flight.
    heads_after: Option<String>,
    /// Entries received in the current exchange, and its cap.
    round_entries: usize,
    round_cap: usize,
    want_queue: BTreeMap<AuthorId, WantPlan>,
    want_inflight: Option<Vec<InflightWant>>,
    unavailable: BTreeSet<AuthorId>,
    evid_queue: BTreeSet<AuthorId>,
    evid_inflight: Option<Vec<AuthorId>>,
    body_queue: BTreeSet<RecordId>,
    body_asked: BTreeSet<RecordId>,
    body_scan: Option<Option<RecordId>>,
    body_inflight: Option<Vec<RecordId>>,
    peers_inflight: bool,
    peers_asked: bool,
    peer_has: BTreeMap<AuthorId, u64>,
    peer_pair: BTreeMap<AuthorId, [RecordId; 2]>,
    peer_hint: BTreeMap<AuthorId, [RecordId; 2]>,
    peer_lacks_body: BTreeSet<RecordId>,
    shareable: Vec<String>,
    learned: Vec<String>,
    score: u32,
    pub stats: Stats,
}

impl Session {
    /// A new session. `outbound` is true when the local peer dialed.
    pub fn new(id: u64, cfg: SessionConfig, outbound: bool) -> Session {
        let send = cfg.limits;
        Session {
            id,
            cfg,
            outbound,
            state: State::AwaitHello,
            peer: None,
            send,
            heads_inflight: false,
            heads_rounds: 0,
            heads_after: None,
            round_entries: 0,
            round_cap: 0,
            want_queue: BTreeMap::new(),
            want_inflight: None,
            unavailable: BTreeSet::new(),
            evid_queue: BTreeSet::new(),
            evid_inflight: None,
            body_queue: BTreeSet::new(),
            body_asked: BTreeSet::new(),
            body_scan: None,
            body_inflight: None,
            peers_inflight: false,
            peers_asked: false,
            peer_has: BTreeMap::new(),
            peer_pair: BTreeMap::new(),
            peer_hint: BTreeMap::new(),
            peer_lacks_body: BTreeSet::new(),
            shareable: Vec::new(),
            learned: Vec::new(),
            score: 0,
            stats: Stats::default(),
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn is_closed(&self) -> bool {
        self.state == State::Closed
    }

    pub fn is_outbound(&self) -> bool {
        self.outbound
    }

    /// The peer's `hello`, once received.
    pub fn peer_hello(&self) -> Option<&Hello> {
        self.peer.as_ref()
    }

    pub fn score(&self) -> u32 {
        self.score
    }

    /// Number of completed heads exchanges.
    pub fn heads_rounds(&self) -> u64 {
        self.heads_rounds
    }

    /// Addresses this session may share in answer to `get_peers`.
    pub fn set_shareable_addresses(&mut self, addrs: Vec<String>) {
        self.shareable = addrs;
    }

    /// Addresses received from the peer since the last call.
    pub fn take_learned_addresses(&mut self) -> Vec<String> {
        std::mem::take(&mut self.learned)
    }

    /// Marks the session closed (transport closed).
    pub fn close(&mut self) {
        self.state = State::Closed;
    }

    /// The first message to send: `hello`.
    pub fn start(&mut self) -> Vec<Message> {
        self.count_sent(vec![Message::Hello(self.cfg.hello())])
    }

    /// `(behind, ahead)`: authors whose head at the peer is lower than local, and the reverse.
    pub fn behind_ahead<S: Store + ?Sized>(&self, store: &S) -> (usize, usize) {
        let mut behind = 0;
        let mut ahead = 0;
        for h in store.heads(None, usize::MAX) {
            let p = self.peer_has.get(&h.author).copied().unwrap_or(0);
            if p < h.seq {
                behind += 1;
            }
        }
        for (a, p) in &self.peer_has {
            let l = store.head(a).map_or(0, |h| h.seq);
            if *p > l {
                ahead += 1;
            }
        }
        (behind, ahead)
    }

    /// Starts a new heads exchange (for example every 10 minutes, or after an admission tier change).
    pub fn refresh<S: Store + ?Sized>(&mut self, store: &S) -> Vec<Message> {
        if !matches!(self.state, State::Syncing | State::Live) {
            return Vec::new();
        }
        self.unavailable.clear();
        self.body_asked.clear();
        let mut out = Out::default();
        if !self.heads_inflight {
            self.start_round(store);
            self.request_heads(None, &mut out);
        }
        self.pump(store, &mut out);
        self.finish(out)
    }

    fn start_round<S: Store + ?Sized>(&mut self, store: &S) {
        self.round_entries = 0;
        self.round_cap = store.author_count().saturating_add(self.cfg.max_extra_authors);
    }

    fn count_sent(&mut self, msgs: Vec<Message>) -> Vec<Message> {
        for m in &msgs {
            self.stats.messages_sent += 1;
            self.stats.records_sent += m.record_count() as u64;
        }
        msgs
    }

    fn fail(&mut self, code: ErrorCode, detail: impl Into<String>) -> SyncError {
        self.state = State::Closed;
        SyncError::new(code, detail)
    }

    fn penalize(&mut self, points: u32, why: &str) -> Result<(), SyncError> {
        self.score = self.score.saturating_add(points);
        if self.score >= score::LIMIT {
            return Err(self.fail(ErrorCode::Misbehavior, format!("peer score {} ({why})", self.score)));
        }
        Ok(())
    }

    fn request_heads(&mut self, after: Option<String>, out: &mut Out) {
        self.heads_inflight = true;
        self.heads_after = after.clone();
        out.msgs.push(Message::GetHeads { after, limit: self.cfg.limits.max_heads.min(limits::MAX_HEADS) });
    }

    /// Handles one received text frame.
    pub fn on_message<S: Store + ?Sized>(&mut self, store: &mut S, text: &str) -> Result<Vec<Message>, SyncError> {
        if self.state == State::Closed {
            return Ok(Vec::new());
        }
        self.stats.messages_received += 1;
        if text.len() > self.cfg.limits.max_message_bytes {
            return Err(self.fail(ErrorCode::TooLarge, format!("message of {} bytes", text.len())));
        }
        let msg = match Message::parse(text) {
            Ok(m) => m,
            Err(e) => {
                self.score = self.score.saturating_add(score::BAD_MESSAGE);
                let code = match e {
                    ParseError::TooLarge(_) => ErrorCode::TooLarge,
                    ParseError::Bad(_) => ErrorCode::BadMessage,
                };
                return Err(self.fail(code, e.to_string()));
            }
        };
        self.stats.records_received += msg.record_count() as u64;
        let mut out = Out::default();
        self.handle(store, msg, &mut out)?;
        if matches!(self.state, State::Syncing | State::Live) {
            self.pump(store, &mut out);
        }
        Ok(self.finish(out))
    }

    fn finish(&mut self, mut out: Out) -> Vec<Message> {
        // Unsolicited data goes last, split by the peer's limits.
        let max_records = self.send.max_records.max(1);
        let budget = self.send.max_message_bytes.saturating_sub(256);
        let mut recs = std::mem::take(&mut out.push_records).into_iter().peekable();
        let mut pairs = std::mem::take(&mut out.push_pairs).into_iter().peekable();
        while recs.peek().is_some() || pairs.peek().is_some() {
            let mut records = Vec::new();
            let mut evidence = Vec::new();
            let mut size = 0usize;
            while let Some(r) = recs.peek() {
                if records.len() >= max_records || (size + r.len() + 1 > budget && !records.is_empty()) {
                    break;
                }
                size += r.len() + 1;
                records.push(recs.next().unwrap_or_default());
            }
            while let Some([a, b]) = pairs.peek() {
                let n = a.len() + b.len() + 4;
                if evidence.len() >= limits::MAX_PAIRS
                    || (size + n > budget && (!records.is_empty() || !evidence.is_empty()))
                {
                    break;
                }
                size += n;
                evidence.push(pairs.next().unwrap_or_default());
            }
            out.msgs.push(Message::Push { records, evidence });
        }
        self.count_sent(out.msgs)
    }

    fn handle<S: Store + ?Sized>(&mut self, store: &mut S, msg: Message, out: &mut Out) -> Result<(), SyncError> {
        if self.state == State::AwaitHello && !matches!(msg, Message::Hello(_) | Message::Error { .. }) {
            self.score = self.score.saturating_add(score::BAD_MESSAGE);
            return Err(self.fail(ErrorCode::BadMessage, format!("{} before hello", msg.type_name())));
        }
        match msg {
            Message::Hello(h) => {
                self.start_round(store);
                self.on_hello(h, out)
            }
            Message::GetHeads { after, limit } => {
                self.answer_heads(store, after, limit, out);
                Ok(())
            }
            Message::Heads { heads, next } => self.on_heads(store, heads, next, out),
            Message::Want { wants } => {
                self.answer_want(store, wants, out);
                Ok(())
            }
            Message::Records { records, truncated } => self.on_records(store, records, truncated, out),
            Message::Push { records, evidence } => {
                for r in &records {
                    self.absorb(store, r, out)?;
                }
                for [a, b] in &evidence {
                    self.absorb_pair(store, a, b, out)?;
                }
                Ok(())
            }
            Message::WantEvidence { authors } => {
                let mut pairs = Vec::new();
                for a in authors.iter().filter_map(|a| author_of(a)) {
                    if let Some(p) = store.evidence(&a) {
                        self.peer_pair.insert(a, p.ids());
                        pairs.push(pair_wire(&p));
                    }
                }
                out.msgs.push(Message::Evidence { pairs });
                Ok(())
            }
            Message::Evidence { pairs } => self.on_evidence(store, pairs, out),
            Message::WantBodies { ids } => {
                self.answer_bodies(store, ids, out);
                Ok(())
            }
            Message::Bodies { records, .. } => {
                if self.body_inflight.take().is_none() {
                    return self.penalize(score::UNSOLICITED_RESPONSE, "bodies without request");
                }
                for r in &records {
                    self.absorb(store, r, out)?;
                }
                Ok(())
            }
            Message::GetPeers => {
                let peers: Vec<String> = self.shareable.iter().take(limits::MAX_PEERS).cloned().collect();
                out.msgs.push(Message::Peers { peers });
                Ok(())
            }
            Message::Peers { peers } => {
                if !std::mem::replace(&mut self.peers_inflight, false) {
                    return self.penalize(score::UNSOLICITED_RESPONSE, "peers without request");
                }
                for p in peers {
                    if crate::message::is_valid_address(&p) && self.learned.len() < 250 {
                        self.learned.push(p);
                    }
                }
                Ok(())
            }
            Message::Error { fatal, .. } => {
                if fatal {
                    self.state = State::Closed;
                }
                Ok(())
            }
            Message::Unknown { .. } => self.penalize(score::UNKNOWN_TYPE, "unknown message type"),
        }
    }

    fn on_hello(&mut self, h: Hello, out: &mut Out) -> Result<(), SyncError> {
        if self.peer.is_some() {
            self.score = self.score.saturating_add(score::BAD_MESSAGE);
            return Err(self.fail(ErrorCode::BadMessage, "second hello"));
        }
        if !h.versions.contains(&VERSION) {
            return Err(self.fail(ErrorCode::UnsupportedVersion, format!("peer speaks {:?}, we speak [{VERSION}]", h.versions)));
        }
        if h.node.eq_ignore_ascii_case(&self.cfg.node) {
            return Err(self.fail(ErrorCode::SelfConnection, "connected to self"));
        }
        self.send = self.cfg.limits.min(h.limits);
        self.peer = Some(h);
        self.state = State::Syncing;
        self.request_heads(None, out);
        Ok(())
    }

    fn head_entry(h: &Head) -> HeadEntry {
        HeadEntry {
            author: h.author.to_hex(),
            seq: h.seq,
            id: h.id.map(|i| i.to_hex()),
            forked: h.forked(),
            evidence: h.evidence.map(|[a, b]| [a.to_hex(), b.to_hex()]),
        }
    }

    fn answer_heads<S: Store + ?Sized>(&mut self, store: &S, after: Option<String>, limit: usize, out: &mut Out) {
        let limit = limit.min(self.send.max_heads).clamp(1, limits::MAX_HEADS);
        let after = after.as_deref().and_then(author_of);
        let mut page = store.heads(after.as_ref(), limit + 1);
        let more = page.len() > limit;
        page.truncate(limit);
        let next = if more { page.last().map(|h| h.author.to_hex()) } else { None };
        let heads = page.iter().map(Self::head_entry).collect();
        out.msgs.push(Message::Heads { heads, next });
    }

    fn on_heads<S: Store + ?Sized>(
        &mut self,
        store: &mut S,
        heads: Vec<HeadEntry>,
        next: Option<String>,
        out: &mut Out,
    ) -> Result<(), SyncError> {
        if !std::mem::replace(&mut self.heads_inflight, false) {
            return self.penalize(score::UNSOLICITED_RESPONSE, "heads without request");
        }
        // Pages must advance: every entry after the requested cursor, and
        // `next` equal to the last entry. This rules out endless paging.
        if let Some(after) = &self.heads_after
            && heads.first().is_some_and(|h| h.author.as_str() <= after.as_str())
        {
            self.score = self.score.saturating_add(score::BAD_MESSAGE);
            return Err(self.fail(ErrorCode::BadMessage, "heads page does not start after the requested author"));
        }
        if let Some(n) = &next
            && heads.last().map(|h| &h.author) != Some(n)
        {
            self.score = self.score.saturating_add(score::BAD_MESSAGE);
            return Err(self.fail(ErrorCode::BadMessage, "heads.next must be the last author of a non-empty page"));
        }
        self.round_entries = self.round_entries.saturating_add(heads.len());
        if self.round_entries > self.round_cap {
            self.score = self.score.saturating_add(score::BAD_MESSAGE);
            return Err(self.fail(
                ErrorCode::Misbehavior,
                format!("heads exchange exceeds {} entries (local authors plus {})", self.round_cap, self.cfg.max_extra_authors),
            ));
        }
        for e in heads {
            let Some(author) = author_of(&e.author) else { continue };
            let peer_id = e.id.as_deref().and_then(|s| RecordId::from_hex(s).ok());
            let ph = self.peer_has.entry(author).or_insert(0);
            *ph = (*ph).max(e.seq);
            let local = store.head(&author);
            let (seq_l, id_l, fork_l, pair_l) = match &local {
                Some(h) => (h.seq, h.id, h.fork_seq, h.evidence),
                None => (0, None, None, None),
            };
            let cap = fork_l.map_or(u64::MAX, |n| n - 1);
            let seq_p = e.seq.min(cap);
            let to = fork_l.map(|n| n - 1);
            if seq_p > seq_l {
                self.queue_want(author, seq_l + 1, to);
            } else if seq_p == seq_l && seq_l > 0 && (e.seq > seq_p || (peer_id.is_some() && peer_id != id_l)) {
                // Same seq with another ID is a fork. When capped by our fork,
                // compare the record at our head too: it can expose a lower fork.
                self.queue_want(author, seq_l, to);
            }
            let peer_ev = e.evidence.as_ref().and_then(|[a, b]| Some([RecordId::from_hex(a).ok()?, RecordId::from_hex(b).ok()?]));
            if let Some(ids) = peer_ev {
                self.peer_pair.insert(author, ids);
                if pair_l != Some(ids) {
                    self.peer_hint.insert(author, ids);
                    self.evid_queue.insert(author);
                }
            } else if e.forked && pair_l.is_none() {
                self.evid_queue.insert(author);
            }
            if pair_l.is_some() && pair_l != peer_ev {
                self.send_pair(store, &author, out);
            }
        }
        match next.as_deref().and_then(author_of) {
            Some(n) => self.request_heads(Some(n.to_hex()), out),
            None => {
                self.heads_rounds += 1;
                if self.state == State::Syncing {
                    self.state = State::Live;
                }
                self.body_scan = Some(None);
            }
        }
        Ok(())
    }

    fn queue_want(&mut self, author: AuthorId, from: u64, to: Option<u64>) {
        if self.unavailable.contains(&author) {
            return;
        }
        if let Some(to) = to
            && from > to
        {
            return;
        }
        let e = self.want_queue.entry(author).or_insert(WantPlan { from, to });
        e.from = e.from.min(from);
        e.to = match (e.to, to) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }

    fn send_pair<S: Store + ?Sized>(&mut self, store: &S, author: &AuthorId, out: &mut Out) {
        if let Some(p) = store.evidence(author)
            && self.peer_pair.get(author) != Some(&p.ids())
        {
            self.peer_pair.insert(*author, p.ids());
            out.push_pairs.push(pair_wire(&p));
        }
    }

    fn answer_want<S: Store + ?Sized>(&mut self, store: &S, wants: Vec<WantEntry>, out: &mut Out) {
        let max_records = self.send.max_records.max(1);
        let budget = self.send.max_message_bytes.saturating_sub(256);
        let mut records: Vec<String> = Vec::new();
        let mut size = 0usize;
        let mut truncated = false;
        'outer: for w in wants {
            let Some(author) = author_of(&w.author) else { continue };
            let Some(head) = store.head(&author) else { continue };
            let last = head.seq.min(w.to_seq.unwrap_or(u64::MAX));
            let mut seq = w.from_seq;
            while seq <= last {
                if records.len() >= max_records {
                    truncated = true;
                    break 'outer;
                }
                let batch = store.log(&author, seq, (max_records - records.len()).min((last - seq + 1) as usize));
                if batch.is_empty() {
                    break;
                }
                for r in batch {
                    if r.seq() > last {
                        break;
                    }
                    let j = r.to_json_string();
                    if size + j.len() + 1 > budget && !records.is_empty() {
                        truncated = true;
                        break 'outer;
                    }
                    size += j.len() + 1;
                    seq = r.seq() + 1;
                    let ph = self.peer_has.entry(author).or_insert(0);
                    *ph = (*ph).max(r.seq());
                    records.push(j);
                }
            }
        }
        out.msgs.push(Message::Records { records, truncated });
    }

    fn answer_bodies<S: Store + ?Sized>(&mut self, store: &S, ids: Vec<String>, out: &mut Out) {
        let max_records = self.send.max_records.max(1);
        let budget = self.send.max_message_bytes.saturating_sub(256);
        let mut records = Vec::new();
        let mut size = 0usize;
        let mut truncated = false;
        for id in ids.iter().filter_map(|s| RecordId::from_hex(s).ok()) {
            match store.get(&id) {
                Some(r) if r.has_body() && !store.is_removed(&id) => {
                    let j = r.to_json_string();
                    if records.len() >= max_records || (size + j.len() + 1 > budget && !records.is_empty()) {
                        truncated = true;
                        break;
                    }
                    size += j.len() + 1;
                    records.push(j);
                    self.peer_lacks_body.remove(&id);
                }
                _ => {
                    if self.peer_lacks_body.len() < MAX_LACKS_BODY {
                        self.peer_lacks_body.insert(id);
                    }
                }
            }
        }
        out.msgs.push(Message::Bodies { records, truncated });
    }

    fn bump(&mut self, author: AuthorId, seq: u64) {
        let ph = self.peer_has.entry(author).or_insert(0);
        *ph = (*ph).max(seq);
    }

    /// Verifies and inserts one received record.
    fn absorb<S: Store + ?Sized>(&mut self, store: &mut S, json: &str, out: &mut Out) -> Result<Option<Insert>, SyncError> {
        let r = match Record::from_json_str(json) {
            Ok(r) => r,
            Err(e) => {
                self.penalize(score::INVALID_RECORD, &format!("invalid record: {e}"))?;
                return Ok(None);
            }
        };
        let author = r.author();
        let seq = r.seq();
        let id = r.id();
        let had_body = r.has_body();
        let o = store.insert(r);
        if o.changed() {
            self.stats.records_accepted += 1;
        }
        match &o {
            Insert::Inserted | Insert::Duplicate | Insert::BodyFilled => self.bump(author, seq),
            Insert::Gap => {
                self.bump(author, seq);
                let head = store.head(&author).map_or(0, |h| h.seq);
                let to = store.fork_seq(&author).map(|n| n - 1);
                if self.want_inflight.as_ref().is_none_or(|w| !w.iter().any(|x| x.author == author)) {
                    self.queue_want(author, head + 1, to);
                }
            }
            Insert::Fork { pair_changed } => {
                if !pair_changed {
                    self.send_pair(store, &author, out);
                }
            }
            Insert::AboveFork => self.send_pair(store, &author, out),
            Insert::Rejected(Reject::TsDecrease { .. }) => self.penalize(score::LOG_REJECTED, "ts decrease")?,
            Insert::Rejected(Reject::Admission(_)) => {
                self.unavailable.insert(author);
                self.want_queue.remove(&author);
            }
            Insert::Rejected(Reject::NotEvidence(_)) => {}
        }
        if !had_body && self.peer_lacks_body.len() < MAX_LACKS_BODY {
            self.peer_lacks_body.insert(id);
        }
        Ok(Some(o))
    }

    fn absorb_pair<S: Store + ?Sized>(&mut self, store: &mut S, a: &str, b: &str, out: &mut Out) -> Result<Option<AuthorId>, SyncError> {
        let (ra, rb) = match (Record::from_json_str(a), Record::from_json_str(b)) {
            (Ok(x), Ok(y)) => (x, y),
            (Err(e), _) | (_, Err(e)) => {
                self.penalize(score::INVALID_RECORD, &format!("invalid evidence record: {e}"))?;
                return Ok(None);
            }
        };
        let author = ra.author();
        let ids = {
            let mut v = [ra.id(), rb.id()];
            v.sort();
            v
        };
        match store.insert_evidence(ra, rb) {
            Insert::Rejected(Reject::NotEvidence(_)) => {
                self.penalize(score::FALSE_FORK_HINT, "pair is not fork evidence")?;
                return Ok(None);
            }
            Insert::Fork { pair_changed } => {
                self.stats.records_accepted += u64::from(pair_changed);
                self.peer_pair.insert(author, ids);
                self.peer_hint.remove(&author);
                if !pair_changed {
                    self.send_pair(store, &author, out);
                }
            }
            _ => {}
        }
        Ok(Some(author))
    }

    fn on_records<S: Store + ?Sized>(&mut self, store: &mut S, records: Vec<String>, truncated: bool, out: &mut Out) -> Result<(), SyncError> {
        let Some(inflight) = self.want_inflight.take() else {
            return self.penalize(score::UNSOLICITED_RESPONSE, "records without request");
        };
        for r in &records {
            self.absorb(store, r, out)?;
        }
        for w in inflight {
            let h = store.head(&w.author);
            let head = h.as_ref().map_or(0, |h| h.seq);
            let fork = h.as_ref().and_then(|h| h.fork_seq);
            let cap = fork.map_or(u64::MAX, |n| n - 1);
            let target = self.peer_has.get(&w.author).copied().unwrap_or(0).min(cap);
            if head >= target || self.want_queue.contains_key(&w.author) {
                continue;
            }
            let progressed = head > w.head_before || fork != w.fork_before;
            if truncated || progressed {
                self.queue_want(w.author, head + 1, fork.map(|n| n - 1));
            } else {
                self.unavailable.insert(w.author);
            }
        }
        Ok(())
    }

    fn on_evidence<S: Store + ?Sized>(&mut self, store: &mut S, pairs: Vec<[String; 2]>, out: &mut Out) -> Result<(), SyncError> {
        let Some(asked) = self.evid_inflight.take() else {
            return self.penalize(score::UNSOLICITED_RESPONSE, "evidence without request");
        };
        let mut got = BTreeSet::new();
        for [a, b] in &pairs {
            if let Some(author) = self.absorb_pair(store, a, b, out)? {
                got.insert(author);
            }
        }
        for a in asked {
            if !got.contains(&a) && self.peer_hint.remove(&a).is_some() {
                self.peer_pair.remove(&a);
                self.penalize(score::FALSE_FORK_HINT, "forked hint without evidence")?;
            }
        }
        Ok(())
    }

    fn needs_body<S: Store + ?Sized>(store: &S, id: &RecordId) -> bool {
        match store.get(id) {
            Some(r) => {
                !r.has_body()
                    && !store.is_removed(id)
                    && store.record_at(&r.author(), r.seq()).is_some_and(|x| x.id() == *id)
            }
            None => false,
        }
    }

    /// Sends pending requests (one of each type in flight).
    fn pump<S: Store + ?Sized>(&mut self, store: &S, out: &mut Out) {
        if self.want_inflight.is_none() && !self.want_queue.is_empty() {
            let mut wants = Vec::new();
            let mut inflight = Vec::new();
            while wants.len() < limits::MAX_WANTS {
                let Some((author, plan)) = self.want_queue.pop_first() else { break };
                if self.unavailable.contains(&author) {
                    continue;
                }
                let h = store.head(&author);
                let head = h.as_ref().map_or(0, |h| h.seq);
                let fork = h.as_ref().and_then(|h| h.fork_seq);
                let to = match (plan.to, fork.map(|n| n - 1)) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                let from = plan.from.max(1).min(head + 1);
                if to.is_some_and(|t| from > t) {
                    continue;
                }
                wants.push(WantEntry { author: author.to_hex(), from_seq: from, to_seq: to });
                inflight.push(InflightWant { author, head_before: head, fork_before: fork });
            }
            if !wants.is_empty() {
                self.want_inflight = Some(inflight);
                out.msgs.push(Message::Want { wants });
            }
        }
        if self.evid_inflight.is_none() && !self.evid_queue.is_empty() {
            let mut authors = Vec::new();
            while authors.len() < limits::MAX_EVIDENCE_AUTHORS {
                let Some(a) = self.evid_queue.pop_first() else { break };
                authors.push(a);
            }
            out.msgs.push(Message::WantEvidence { authors: authors.iter().map(|a| a.to_hex()).collect() });
            self.evid_inflight = Some(authors);
        }
        if self.state == State::Live && self.body_inflight.is_none() {
            let mut ids: Vec<RecordId> = Vec::new();
            while ids.len() < limits::MAX_BODY_IDS {
                let Some(id) = self.body_queue.pop_first() else { break };
                if !self.body_asked.contains(&id) && Self::needs_body(store, &id) {
                    ids.push(id);
                }
            }
            while ids.len() < limits::MAX_BODY_IDS {
                let Some(cursor) = self.body_scan else { break };
                let page = store.missing_bodies(cursor.as_ref(), limits::MAX_BODY_IDS);
                match page.last() {
                    None => {
                        self.body_scan = None;
                        break;
                    }
                    Some(last) => self.body_scan = Some(Some(*last)),
                }
                for id in page {
                    if ids.len() < limits::MAX_BODY_IDS && !self.body_asked.contains(&id) && !self.peer_lacks_body.contains(&id) {
                        ids.push(id);
                    }
                }
            }
            ids.sort();
            ids.dedup();
            if !ids.is_empty() {
                self.body_asked.extend(ids.iter().copied());
                out.msgs.push(Message::WantBodies { ids: ids.iter().map(|i| i.to_hex()).collect() });
                self.body_inflight = Some(ids);
            }
        }
        if self.state == State::Live && !self.peers_asked {
            self.peers_asked = true;
            self.peers_inflight = true;
            out.msgs.push(Message::GetPeers);
        }
    }

    /// Store changes made by any session or by a local submission.
    /// `origin` is the session that caused them, or `None` for local ones.
    pub fn on_changes<S: Store + ?Sized>(&mut self, store: &S, changes: &[Change], origin: Option<u64>) -> Vec<Message> {
        if !matches!(self.state, State::Syncing | State::Live) {
            return Vec::new();
        }
        let mine = origin == Some(self.id);
        let mut out = Out::default();
        for c in changes {
            match c {
                Change::Inserted(r) => {
                    let a = r.author();
                    let ph = self.peer_has.get(&a).copied().unwrap_or(0);
                    if ph < r.seq() {
                        self.peer_has.insert(a, r.seq());
                        out.push_records.push(r.to_json_string());
                        if !r.has_body() && self.peer_lacks_body.len() < MAX_LACKS_BODY {
                            self.peer_lacks_body.insert(r.id());
                        }
                    } else if r.has_body() && self.peer_lacks_body.remove(&r.id()) {
                        out.push_records.push(r.to_json_string());
                    }
                    if !r.has_body() && !mine && !store.is_removed(&r.id()) {
                        self.body_queue.insert(r.id());
                    }
                }
                Change::BodyFilled(r) => {
                    if !mine
                        && (self.peer_lacks_body.remove(&r.id())
                            || self.peer_has.get(&r.author()).copied().unwrap_or(0) >= r.seq())
                    {
                        out.push_records.push(r.to_json_string());
                    }
                }
                Change::PairChanged(p) => self.send_pair(store, &p.author(), &mut out),
                Change::BodyRemoved(_) => {}
            }
        }
        self.pump(store, &mut out);
        self.finish(out)
    }
}
