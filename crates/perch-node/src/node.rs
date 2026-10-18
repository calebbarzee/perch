//! Shared node state and the operations HTTP, admin, and sync share.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use perch_core::kinds::{self, names};
use perch_core::{AuthorId, Keypair, Record, RecordId, Unsigned, sign_record};
use perch_store::{Change, Insert, MemoryStore, Mode, Store};
use perch_sync::{AddressBook, Role, SessionConfig};
use perch_views::Policy;
use serde_json::{Map, Value, json};
use tokio::sync::{Notify, broadcast};

use crate::admission::Admission;
use crate::config::ServeConfig;

/// Current time in Unix milliseconds.
pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Store changes caused by one session (`origin`) or by a local submission (`None`).
#[derive(Debug)]
pub struct Batch {
    pub origin: Option<u64>,
    pub changes: Vec<Change>,
}

/// What `/v1/status` shows about one session.
#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub session: u64,
    pub outbound: bool,
    pub url: Option<String>,
    pub remote: Option<SocketAddr>,
    pub node: Option<String>,
    pub role: Option<&'static str>,
    pub state: &'static str,
    pub messages_sent: u64,
    pub messages_received: u64,
    pub records_sent: u64,
    pub records_received: u64,
    pub behind: usize,
    pub ahead: usize,
    pub score: u32,
    pub kill: Arc<Notify>,
}

impl PeerInfo {
    pub fn to_json(&self) -> Value {
        json!({
            "session": self.session,
            "direction": if self.outbound { "outbound" } else { "inbound" },
            "url": self.url,
            "remote": self.remote.map(|r| r.to_string()),
            "node": self.node,
            "role": self.role,
            "state": self.state,
            "messages_sent": self.messages_sent,
            "messages_received": self.messages_received,
            "records_sent": self.records_sent,
            "records_received": self.records_received,
            "behind": self.behind,
            "ahead": self.ahead,
            "score": self.score,
        })
    }
}

/// Software and build identification.
#[derive(Clone, Debug)]
pub struct BuildInfo {
    pub version: &'static str,
    /// SHA-256 of the running executable file.
    pub build_hash: String,
}

/// Errors of node operations.
#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Internal(String),
}

/// The node.
pub struct Node {
    store: RwLock<MemoryStore>,
    pub policy: Policy,
    pub policy_hash: String,
    pub node_id: String,
    pub sync_cfg: SessionConfig,
    pub changes: broadcast::Sender<Arc<Batch>>,
    pub peers: Mutex<BTreeMap<u64, PeerInfo>>,
    next_session: AtomicU64,
    pub addresses: Mutex<AddressBook>,
    pub build: BuildInfo,
    pub started: Instant,
    pub started_ms: u64,
    pub removed_path: PathBuf,
    pub operator: Option<Keypair>,
    pub config: ServeConfig,
    pub admission: Option<Arc<Admission>>,
}

fn random_hex(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    if getrandom::fill(&mut b).is_err() {
        let t = now_ms().to_le_bytes();
        for (i, x) in b.iter_mut().enumerate() {
            *x = t[i % 8] ^ (i as u8).wrapping_mul(31);
        }
    }
    perch_core::ids::encode_hex(&b)
}

/// Reads a removal list: one record ID per line; `#` starts a comment.
pub fn read_removed(path: &std::path::Path) -> std::io::Result<Vec<RecordId>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let l = line.split('#').next().unwrap_or("").trim();
        if l.is_empty() {
            continue;
        }
        match RecordId::from_hex(l) {
            Ok(id) => out.push(id),
            Err(e) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{}:{}: {e}", path.display(), i + 1),
                ));
            }
        }
    }
    Ok(out)
}

impl Node {
    /// Opens the store under `config.data`, replays it, and builds the node.
    pub fn open(config: ServeConfig, policy: Policy, build: BuildInfo) -> Result<Node, String> {
        std::fs::create_dir_all(&config.data).map_err(|e| format!("{}: {e}", config.data.display()))?;
        let removed_path = config.data.join("removed.txt");
        let mut removed = read_removed(&removed_path).map_err(|e| e.to_string())?;
        for f in &config.removed_files {
            removed.extend(read_removed(f).map_err(|e| e.to_string())?);
        }
        let path = config.data.join("records.jsonl");
        let t0 = Instant::now();
        let (mut store, report) = MemoryStore::open(&path, Mode::Full, removed).map_err(|e| e.to_string())?;
        tracing::info!(
            lines = report.lines,
            applied = report.applied,
            invalid = report.invalid.len(),
            truncated_bytes = report.truncated_bytes,
            compacted = report.compacted,
            ms = t0.elapsed().as_millis() as u64,
            "replayed {}",
            path.display()
        );
        for (line, e) in report.invalid.iter().take(10) {
            tracing::warn!(line, error = %e, "invalid line in persistence file");
        }
        store.set_track_changes(true);
        let admission =
            if config.open_admission { None } else { Some(Arc::new(Admission::new(policy.clone(), config.tiers))) };
        if let Some(a) = &admission {
            let a2 = a.clone();
            store.set_admission(Some(Arc::new(move |s: &MemoryStore, author: &AuthorId| a2.limit(s, author))));
            store.set_pool_budget(a.limits.t3_budget);
            let t = Instant::now();
            let dropped = store.rebuild_pool(true);
            if !dropped.is_empty() {
                store.compact().map_err(|e| format!("compaction: {e}"))?;
            }
            tracing::info!(
                pool_authors = store.pool_len(),
                pool_bytes = store.pool_bytes(),
                dropped = dropped.len(),
                ms = t.elapsed().as_millis() as u64,
                "admission tiers applied"
            );
        }
        store.drain_changes();
        let node_id = random_hex(16);
        let mut sync_cfg = SessionConfig::new(node_id.clone(), Role::Node);
        sync_cfg.software = format!("perch-node/{}", build.version);
        sync_cfg.listen = config.public_url.clone();
        let mut book = AddressBook::new();
        for p in &config.peers {
            book.add_configured(p);
        }
        let operator = match &config.operator_key {
            Some(p) => {
                let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
                let hex = text.trim();
                let hex = serde_json::from_str::<Value>(hex)
                    .ok()
                    .and_then(|v| v.get("secret").and_then(|s| s.as_str()).map(str::to_owned))
                    .unwrap_or_else(|| hex.to_owned());
                Some(Keypair::from_seed_hex(&hex).map_err(|e| format!("operator key: {e}"))?)
            }
            None => None,
        };
        let (changes, _) = broadcast::channel(1024);
        let policy_hash = perch_views::policy_hash(&policy);
        Ok(Node {
            store: RwLock::new(store),
            policy,
            policy_hash,
            node_id,
            sync_cfg,
            changes,
            peers: Mutex::new(BTreeMap::new()),
            next_session: AtomicU64::new(1),
            addresses: Mutex::new(book),
            build,
            started: Instant::now(),
            started_ms: now_ms(),
            removed_path,
            operator,
            config,
            admission,
        })
    }

    pub fn read(&self) -> RwLockReadGuard<'_, MemoryStore> {
        self.store.read().unwrap_or_else(|e| e.into_inner())
    }

    pub fn write(&self) -> RwLockWriteGuard<'_, MemoryStore> {
        self.store.write().unwrap_or_else(|e| e.into_inner())
    }

    pub fn next_session_id(&self) -> u64 {
        self.next_session.fetch_add(1, Ordering::Relaxed)
    }

    fn peer_entry(id: u64, outbound: bool, url: Option<String>, remote: Option<SocketAddr>, kill: Arc<Notify>) -> PeerInfo {
        PeerInfo {
            session: id,
            outbound,
            url,
            remote,
            node: None,
            role: None,
            state: "connecting",
            messages_sent: 0,
            messages_received: 0,
            records_sent: 0,
            records_received: 0,
            behind: 0,
            ahead: 0,
            score: 0,
            kill,
        }
    }

    /// Registers an outbound session (counted by `--max-outbound` elsewhere).
    pub fn register_session(&self, outbound: bool, url: Option<String>, remote: Option<SocketAddr>) -> (u64, Arc<Notify>) {
        let id = self.next_session_id();
        let kill = Arc::new(Notify::new());
        self.peers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, Self::peer_entry(id, outbound, url, remote, kill.clone()));
        (id, kill)
    }

    /// Checks the inbound session caps and reserves a slot in one step, so
    /// concurrent upgrades cannot exceed them (audit #14).
    pub fn reserve_inbound(&self, remote: SocketAddr) -> Result<(u64, Arc<Notify>), &'static str> {
        use crate::config::limits;
        let mut peers = self.peers.lock().unwrap_or_else(|e| e.into_inner());
        let inbound = peers.values().filter(|p| !p.outbound).count();
        if inbound >= limits::INBOUND_SESSIONS {
            return Err("inbound session limit reached");
        }
        let same_ip = peers.values().filter(|p| !p.outbound && p.remote.map(|r| r.ip()) == Some(remote.ip())).count();
        // Loopback is exempt from the per-address cap only (local nodes, test browsers).
        if !remote.ip().is_loopback() && same_ip >= limits::INBOUND_PER_IP {
            return Err("at most 8 sessions per address");
        }
        let id = self.next_session_id();
        let kill = Arc::new(Notify::new());
        peers.insert(id, Self::peer_entry(id, false, None, Some(remote), kill.clone()));
        Ok((id, kill))
    }

    pub fn unregister_session(&self, id: u64) {
        self.peers.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    }

    /// The node's policy at `now` (current time if `None`).
    pub fn policy_at(&self, now: Option<u64>) -> Policy {
        let mut p = self.policy.clone();
        p.now = now.unwrap_or_else(now_ms);
        p
    }

    /// Sends a batch of changes to every session, after the store lock is released.
    pub fn publish_changes(&self, origin: Option<u64>, changes: Vec<Change>) {
        if changes.is_empty() {
            return;
        }
        if let Some(a) = &self.admission {
            a.on_changes(&changes);
        }
        let _ = self.changes.send(Arc::new(Batch { origin, changes }));
    }

    /// Inserts locally submitted records, persists them, and gossips the changes.
    pub fn submit(&self, records: Vec<Record>) -> Result<Vec<Insert>, NodeError> {
        let (outcomes, changes) = {
            let mut st = self.write();
            let outcomes = perch_sync::insert_batch(&mut *st, records);
            let changes = st.drain_changes();
            st.flush().map_err(|e| NodeError::Internal(format!("persistence: {e}")))?;
            (outcomes, changes)
        };
        self.publish_changes(None, changes);
        Ok(outcomes)
    }

    /// Periodic work: re-evaluate tiers and the tier-3 pool, and compact the
    /// persistence file if logs were dropped.
    pub fn maintain(&self) {
        let Some(a) = &self.admission else { return };
        a.clear();
        let mut st = self.write();
        let dropped = st.rebuild_pool(false);
        if !dropped.is_empty() {
            tracing::info!(dropped = dropped.len(), "tier-3 logs dropped over the node budget");
        }
        if st.needs_compaction()
            && let Err(e) = st.compact()
        {
            tracing::error!(error = %e, "compaction failed");
        }
    }

    /// Addresses to share with a peer: sampled from the address book, plus our own.
    pub fn shareable(&self, exclude: Option<&str>) -> Vec<String> {
        let mut ex = std::collections::BTreeSet::new();
        if let Some(e) = exclude {
            ex.insert(e.to_string());
        }
        let seed = now_ms();
        let mut v = self.addresses.lock().unwrap_or_else(|e| e.into_inner()).sample(99, seed, &ex);
        if let Some(own) = &self.config.public_url
            && !v.contains(own)
        {
            v.push(own.clone());
        }
        v
    }

    /// Removes the body of `id` (operator control, ARCHITECTURE 5.3), records
    /// the ID in `removed.txt`, and publishes a signed `redact` record when an
    /// operator key is configured.
    pub fn redact(&self, id: RecordId, reason: &str, note: Option<String>) -> Result<Value, NodeError> {
        if kinds::RedactReason::parse(reason).is_none() {
            return Err(NodeError::BadRequest(format!(
                "reason must be one of illegal, personal_data, court_order, other; got {reason:?}"
            )));
        }
        if let Some(n) = &note
            && n.chars().count() > 2000
        {
            return Err(NodeError::BadRequest("note is longer than 2000 characters".into()));
        }
        let mut changes = Vec::new();
        let (dropped, redact) = {
            let mut st = self.write();
            let rec = st.get(&id);
            if let Some(r) = &rec
                && kinds::is_control_kind(r.kind())
            {
                return Err(NodeError::Conflict(format!(
                    "record {id} has the control kind {}; its body cannot be removed (ARCHITECTURE 4.5)",
                    r.kind()
                )));
            }
            let product = rec.as_ref().and_then(|r| product_of(&*st, r));
            let dropped = st.remove_body(&id).map_err(|e| NodeError::Conflict(e.to_string()))?;
            append_removed(&self.removed_path, &id, reason).map_err(|e| NodeError::Internal(e.to_string()))?;
            let mut redact = None;
            if let Some(kp) = &self.operator {
                let author = kp.author();
                let (seq, prev) = st.next_position(&author);
                let prev_ts = prev.and_then(|p| st.get(&p)).map_or(0, |r| r.ts());
                let mut body = Map::new();
                body.insert("target".into(), Value::String(id.to_hex()));
                body.insert("reason".into(), Value::String(reason.to_string()));
                if let Some(n) = note {
                    body.insert("note".into(), Value::String(n));
                }
                if let Some(p) = product {
                    body.insert("product".into(), Value::String(p));
                }
                let r = sign_record(
                    kp,
                    Unsigned { seq, prev, ts: now_ms().max(prev_ts), kind: names::REDACT.into(), replaces: None, body },
                )
                .map_err(|e| NodeError::Internal(e.to_string()))?;
                let o = st.insert(r.clone());
                if o != Insert::Inserted {
                    return Err(NodeError::Internal(format!("redact record not stored: {}", o.name())));
                }
                redact = Some(r);
            }
            changes.extend(st.drain_changes());
            st.flush().map_err(|e| NodeError::Internal(format!("persistence: {e}")))?;
            (dropped, redact)
        };
        self.publish_changes(None, changes);
        tracing::warn!(%id, reason, dropped, "body removed by operator");
        Ok(json!({
            "id": id.to_hex(),
            "body_dropped": dropped,
            "removed_list": self.removed_path.display().to_string(),
            "redact": redact.map(|r| r.to_value()),
        }))
    }
}

/// The product key a record is about, for the `product` field of `redact`.
fn product_of<S: Store + ?Sized>(st: &S, r: &Record) -> Option<String> {
    let body = r.body()?;
    match r.kind() {
        names::LISTING | names::OBSERVATION | names::REVIEW | names::REDACT => {
            body.get("product").and_then(|p| p.as_str()).map(str::to_owned)
        }
        names::PRODUCT => {
            let mut root = r.clone();
            while let Some(rep) = root.replaces() {
                match st.get(&rep) {
                    Some(x) if x.author() == root.author() && x.seq() < root.seq() && x.kind() == root.kind() => root = x,
                    _ => break,
                }
            }
            kinds::derive_product_key(body, root.id()).ok().map(|k| k.as_str().to_owned())
        }
        _ => None,
    }
}

fn append_removed(path: &std::path::Path, id: &RecordId, reason: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}  # {} {}", id.to_hex(), reason, now_ms())?;
    f.sync_data()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_node(name: &str) -> Node {
        let data = std::env::temp_dir().join(format!("perch-node-unit-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let config = ServeConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            data,
            peers: vec![],
            web: None,
            removed_files: vec![],
            admin: None,
            operator_key: None,
            public_url: None,
            open_admission: true,
            refresh_secs: 600,
            max_outbound: 8,
            imports: vec![],
            tiers: Default::default(),
            trust_proxy_header: None,
        };
        let build = BuildInfo { version: "test", build_hash: "x".into() };
        Node::open(config, perch_views::default_policy(0, &[]), build).unwrap()
    }

    /// Audit #14: caps are checked and the slot reserved in one step.
    #[test]
    fn inbound_reservation_enforces_caps() {
        let node = test_node("caps");
        let ip: SocketAddr = "198.51.100.4:5000".parse().unwrap();
        let ids: Vec<u64> = (0..8).map(|_| node.reserve_inbound(ip).unwrap().0).collect();
        assert!(node.reserve_inbound(ip).is_err(), "9th from one address");
        node.unregister_session(ids[0]);
        assert!(node.reserve_inbound(ip).is_ok(), "slot freed");
        let lo: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        let mut n = 8;
        while node.reserve_inbound(lo).is_ok() {
            n += 1;
        }
        assert_eq!(n, crate::config::limits::INBOUND_SESSIONS, "total cap holds, loopback included");
    }
}
