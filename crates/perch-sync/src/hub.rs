//! [`Hub`]: all sessions of one peer plus gossip fan-out, for single-threaded
//! drivers (browser wasm, simulation). A multi-threaded node can use
//! [`Session`] directly and fan out [`Change`]s itself.

use std::collections::BTreeMap;

use perch_core::Record;
use perch_store::{Change, Insert, Store};

use crate::message::{ErrorCode, Message};
use crate::session::{Session, SessionConfig};

/// Messages to send and sessions to close, produced by one hub call.
#[derive(Debug, Default)]
pub struct Outbox {
    /// `(session id, message)` in send order.
    pub send: Vec<(u64, Message)>,
    /// Sessions that must be closed after their messages are sent.
    pub close: Vec<u64>,
}

impl Outbox {
    fn extend(&mut self, id: u64, msgs: Vec<Message>) {
        self.send.extend(msgs.into_iter().map(|m| (id, m)));
    }
}

/// The sessions of one peer.
#[derive(Debug)]
pub struct Hub {
    cfg: SessionConfig,
    sessions: BTreeMap<u64, Session>,
    next_id: u64,
}

impl Hub {
    /// A hub whose sessions use `cfg`. The store must have change tracking on
    /// (`MemoryStore::set_track_changes(true)`), or gossip will not be sent.
    pub fn new(cfg: SessionConfig) -> Hub {
        Hub { cfg, sessions: BTreeMap::new(), next_id: 1 }
    }

    pub fn config(&self) -> &SessionConfig {
        &self.cfg
    }

    /// Opens a session and returns its ID and first messages (`hello`).
    pub fn open(&mut self, outbound: bool) -> (u64, Vec<Message>) {
        let id = self.next_id;
        self.next_id += 1;
        let mut s = Session::new(id, self.cfg.clone(), outbound);
        let hello = s.start();
        self.sessions.insert(id, s);
        (id, hello)
    }

    /// Removes a session (transport closed).
    pub fn close(&mut self, id: u64) -> Option<Session> {
        self.sessions.remove(&id)
    }

    pub fn session(&self, id: u64) -> Option<&Session> {
        self.sessions.get(&id)
    }

    pub fn session_mut(&mut self, id: u64) -> Option<&mut Session> {
        self.sessions.get_mut(&id)
    }

    pub fn sessions(&self) -> impl Iterator<Item = &Session> {
        self.sessions.values()
    }

    /// Handles one received message on session `id`.
    pub fn on_message<S: Store + ?Sized>(&mut self, store: &mut S, id: u64, text: &str) -> Outbox {
        let mut out = Outbox::default();
        let Some(s) = self.sessions.get_mut(&id) else { return out };
        let had_peer = s.peer_hello().is_some();
        match s.on_message(store, text) {
            Ok(msgs) => out.extend(id, msgs),
            Err(e) => {
                out.send.push((id, e.to_message()));
                out.close.push(id);
            }
        }
        if s.is_closed() && !out.close.contains(&id) {
            out.close.push(id);
        }
        if !had_peer && s.peer_hello().is_some() {
            self.check_duplicate(id, &mut out);
        }
        let changes = store_changes(store);
        self.fanout_into(store, changes, Some(id), &mut out);
        out
    }

    /// Two sessions to the same peer process: keep the one dialed by the
    /// peer with the lower node ID, so both ends choose the same session.
    fn check_duplicate(&mut self, id: u64, out: &mut Outbox) {
        let me = self.cfg.node.clone();
        let Some(new) = self.sessions.get(&id) else { return };
        let Some(peer) = new.peer_hello().map(|h| h.node.clone()) else { return };
        let other = self
            .sessions
            .values()
            .find(|s| s.id != id && !s.is_closed() && s.peer_hello().is_some_and(|h| h.node == peer))
            .map(|s| s.id);
        let Some(other) = other else { return };
        let dialer = |s: &Session| if s.is_outbound() { me.clone() } else { peer.clone() };
        let keep_new = dialer(&self.sessions[&id]) < dialer(&self.sessions[&other]);
        let drop = if keep_new { other } else { id };
        if let Some(s) = self.sessions.get_mut(&drop) {
            s.close();
        }
        out.send.push((drop, Message::error(ErrorCode::DuplicateConnection, "another session to this peer exists")));
        out.close.push(drop);
    }

    /// Inserts records submitted locally (HTTP, local publish) and gossips them.
    /// A record not stored on its own is tried as an evidence pair with the record before it.
    pub fn on_local<S: Store + ?Sized>(&mut self, store: &mut S, records: Vec<Record>) -> (Vec<Insert>, Outbox) {
        let outcomes = insert_batch(store, records);
        let mut out = Outbox::default();
        let changes = store_changes(store);
        self.fanout_into(store, changes, None, &mut out);
        (outcomes, out)
    }

    /// Sends store changes to every session.
    pub fn fanout<S: Store + ?Sized>(&mut self, store: &S, changes: Vec<Change>, origin: Option<u64>) -> Outbox {
        let mut out = Outbox::default();
        self.fanout_into(store, changes, origin, &mut out);
        out
    }

    fn fanout_into<S: Store + ?Sized>(&mut self, store: &S, changes: Vec<Change>, origin: Option<u64>, out: &mut Outbox) {
        if changes.is_empty() {
            return;
        }
        for (id, s) in self.sessions.iter_mut() {
            let msgs = s.on_changes(store, &changes, origin);
            out.extend(*id, msgs);
        }
    }

    /// Starts a new heads exchange on every session.
    pub fn refresh<S: Store + ?Sized>(&mut self, store: &S) -> Outbox {
        let mut out = Outbox::default();
        for (id, s) in self.sessions.iter_mut() {
            let msgs = s.refresh(store);
            out.extend(*id, msgs);
        }
        out
    }
}

fn store_changes<S: Store + ?Sized>(store: &mut S) -> Vec<Change> {
    store.drain_changes()
}

/// Inserts records in order; a record not stored on its own is tried as an
/// evidence pair with the record before it (same rule as `MemoryStore::insert_batch`).
pub fn insert_batch<S: Store + ?Sized>(store: &mut S, records: Vec<Record>) -> Vec<Insert> {
    let mut out = Vec::with_capacity(records.len());
    let mut prev: Option<Record> = None;
    for r in records {
        let mut o = store.insert(r.clone());
        if matches!(o, Insert::Gap | Insert::AboveFork | Insert::Rejected(_))
            && let Some(p) = &prev
            && perch_core::fork::fork_type(p, &r).is_ok()
        {
            o = store.insert_evidence(p.clone(), r.clone());
        }
        out.push(o);
        prev = Some(r);
    }
    out
}
