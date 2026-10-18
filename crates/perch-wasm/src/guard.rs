//! Publish guard (ARCHITECTURE 8.2, 10).
//!
//! A client must never sign a record at or below a position it has already
//! used, or it forks its own log. The guard refuses to publish until:
//!
//! 1. the local store holds the author's log up to the highest **proven**
//!    head: from the local store (verified records) or the user's backup;
//! 2. with an imported key, every configured node has finished its heads
//!    exchange since the import;
//! 3. no node claims a higher head that is still pending.
//!
//! A node's head claim is not proof: a malicious node could claim a huge seq
//! and block publishing forever. A claim above the local head is pending
//! while sync requests the records. Verified records raise the local head
//! and settle the claim. A claim that is not proven within
//! [`CLAIM_TIMEOUT_MS`] is ignored, and the status names the node.
//!
//! Pure logic: the caller passes the clock. Tested natively.

use std::collections::BTreeMap;

/// How long a node may take to deliver verified records for a head it claims.
pub const CLAIM_TIMEOUT_MS: u64 = 60_000;

/// A log position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadRef {
    pub seq: u64,
    pub id: String,
}

/// What one node told us about the author's log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeHead {
    pub url: String,
    /// The head the node claims for the author (unverified).
    pub head: Option<HeadRef>,
    /// True once the node's heads were compared after the key was imported.
    pub checked: bool,
    /// When the current claim was first seen, in ms.
    pub claim_since: Option<u64>,
    /// The claim is above the local head and was not proven in time.
    pub unproven: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishStatus {
    pub ready: bool,
    /// Why publishing is blocked, or, when `ready`, a warning about an
    /// ignored claim.
    pub reason: Option<String>,
    pub local_head: Option<HeadRef>,
    pub nodes: Vec<NodeHead>,
}

#[derive(Clone, Debug, Default)]
pub struct Guard {
    /// Highest proven head: verified local records or the user's backup.
    known: Option<HeadRef>,
    /// The key was imported (or restored): another device may have published.
    imported: bool,
    nodes: BTreeMap<String, NodeHead>,
    sparse: bool,
}

fn host_of(url: &str) -> &str {
    let rest = url.split("://").nth(1).unwrap_or(url);
    rest.split('/').next().unwrap_or(rest)
}

impl Guard {
    pub fn new(sparse: bool) -> Guard {
        Guard { sparse, ..Guard::default() }
    }

    /// A fresh key: nothing was published with it anywhere.
    pub fn reset_for_new_key(&mut self) {
        self.known = None;
        self.imported = false;
        for n in self.nodes.values_mut() {
            n.head = None;
            n.checked = true;
            n.claim_since = None;
            n.unproven = false;
        }
    }

    /// An imported key, with the head from its backup if any.
    pub fn reset_for_import(&mut self, backup_head: Option<HeadRef>) {
        self.known = backup_head;
        self.imported = true;
        for n in self.nodes.values_mut() {
            n.head = None;
            n.checked = false;
            n.claim_since = None;
            n.unproven = false;
        }
    }

    /// Raise the proven head (never lowers it). Call only with heads of
    /// verified records or from the user's own backup.
    pub fn note_known(&mut self, head: HeadRef) {
        if self.known.as_ref().is_none_or(|k| head.seq > k.seq) {
            self.known = Some(head);
        }
    }

    pub fn known(&self) -> Option<&HeadRef> {
        self.known.as_ref()
    }

    pub fn add_node(&mut self, url: &str) {
        let imported = self.imported;
        self.nodes.entry(url.to_string()).or_insert_with(|| NodeHead {
            url: url.to_string(),
            head: None,
            checked: !imported,
            claim_since: None,
            unproven: false,
        });
    }

    pub fn remove_node(&mut self, url: &str) {
        self.nodes.remove(url);
    }

    /// A node advertised a head for the author. Unverified: recorded as a claim.
    pub fn node_reported(&mut self, url: &str, head: Option<HeadRef>, now_ms: u64) {
        let Some(n) = self.nodes.get_mut(url) else { return };
        if n.head != head {
            n.claim_since = head.as_ref().map(|_| now_ms);
            n.unproven = false;
            n.head = head;
        }
    }

    /// The node finished its initial heads exchange after the import.
    pub fn node_checked(&mut self, url: &str) {
        if let Some(n) = self.nodes.get_mut(url) {
            n.checked = true;
        }
    }

    /// Computes whether publishing is allowed, given the local store's head
    /// (verified records) and the clock.
    pub fn status(&mut self, has_key: bool, local_head: Option<HeadRef>, now_ms: u64) -> PublishStatus {
        if let Some(h) = local_head.clone() {
            self.note_known(h);
        }
        let local_seq = local_head.as_ref().map_or(0, |h| h.seq);
        let local_id = local_head.as_ref().map(|h| h.id.as_str());

        // Settle or expire node claims.
        let mut pending = Vec::new();
        let mut ignored = Vec::new();
        for n in self.nodes.values_mut() {
            let Some(claim) = &n.head else {
                n.unproven = false;
                continue;
            };
            let settled = claim.seq < local_seq || (claim.seq == local_seq && Some(claim.id.as_str()) == local_id);
            if settled {
                n.unproven = false;
                continue;
            }
            let since = n.claim_since.unwrap_or(now_ms);
            if now_ms.saturating_sub(since) >= CLAIM_TIMEOUT_MS {
                n.unproven = true;
                ignored.push(host_of(&n.url).to_string());
            } else {
                pending.push((host_of(&n.url).to_string(), claim.seq));
            }
        }

        let nodes: Vec<NodeHead> = self.nodes.values().cloned().collect();
        let blocker = self.blocker(has_key, local_head.as_ref(), &pending);
        let reason = match (&blocker, ignored.is_empty()) {
            (Some(b), _) => Some(b.clone()),
            (None, false) => Some(format!(
                "Ignored: {} {} a newer head for your log that {} cannot prove.",
                ignored.join(", "),
                if ignored.len() == 1 { "claims" } else { "claim" },
                if ignored.len() == 1 { "it" } else { "they" },
            )),
            (None, true) => None,
        };
        PublishStatus { ready: blocker.is_none(), reason, local_head, nodes }
    }

    fn blocker(&self, has_key: bool, local: Option<&HeadRef>, pending: &[(String, u64)]) -> Option<String> {
        if self.sparse {
            return Some("This client has a sparse store and cannot publish.".into());
        }
        if !has_key {
            return Some("No key. Generate or import a key first.".into());
        }
        if self.imported {
            let total = self.nodes.len();
            if total == 0 {
                return Some("This key was imported. Connect to a node and wait for sync before publishing.".into());
            }
            let checked = self.nodes.values().filter(|n| n.checked).count();
            if checked < total {
                return Some(format!("This key was imported. Waiting for sync from {} of {} nodes.", total - checked, total));
            }
        }
        let local_seq = local.map_or(0, |h| h.seq);
        if let Some(k) = &self.known {
            if local_seq < k.seq {
                return Some(format!(
                    "Your log has {} records, but this device holds {}. Sync the missing records from a node first.",
                    k.seq, local_seq
                ));
            }
            if local_seq == k.seq && local.map(|h| &h.id) != Some(&k.id) {
                return Some(format!(
                    "Record {} of your log on this device differs from your backup. Publishing now would fork your log.",
                    k.seq
                ));
            }
        }
        if let Some((host, seq)) = pending.first() {
            return Some(format!(
                "Node {host} reports {seq} records in your log; this device holds {local_seq}. Waiting for the node to send them."
            ));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(seq: u64, id: &str) -> HeadRef {
        HeadRef { seq, id: id.into() }
    }

    #[test]
    fn new_key_is_ready() {
        let mut g = Guard::new(false);
        g.add_node("ws://a");
        g.reset_for_new_key();
        assert!(g.status(true, None, 0).ready);
    }

    #[test]
    fn no_key_blocks() {
        let mut g = Guard::new(false);
        assert!(!g.status(false, None, 0).ready);
    }

    #[test]
    fn sparse_blocks() {
        let mut g = Guard::new(true);
        g.reset_for_new_key();
        assert!(g.status(true, None, 0).reason.unwrap().contains("sparse"));
    }

    #[test]
    fn import_waits_for_every_node() {
        let mut g = Guard::new(false);
        g.add_node("ws://a/v1/sync");
        g.add_node("ws://b/v1/sync");
        g.reset_for_import(None);
        assert!(g.status(true, None, 0).reason.unwrap().contains("2 of 2"));
        g.node_reported("ws://a/v1/sync", Some(h(3, "x3")), 0);
        g.node_checked("ws://a/v1/sync");
        assert!(g.status(true, None, 0).reason.unwrap().contains("1 of 2"));
        g.node_checked("ws://b/v1/sync");
        // Node a claims 3 records the device lacks: pending.
        let s = g.status(true, None, 1000);
        assert!(!s.ready);
        assert!(s.reason.unwrap().contains("Node a reports 3 records"));
        // The records arrive and verify: the local head reaches 3.
        assert!(g.status(true, Some(h(3, "x3")), 2000).ready);
    }

    #[test]
    fn import_without_nodes_blocks() {
        let mut g = Guard::new(false);
        g.reset_for_import(None);
        assert!(g.status(true, None, 0).reason.unwrap().contains("Connect to a node"));
    }

    #[test]
    fn backup_head_above_local_blocks() {
        let mut g = Guard::new(false);
        g.add_node("ws://a");
        g.reset_for_import(Some(h(5, "x5")));
        g.node_reported("ws://a", Some(h(4, "x4")), 0);
        g.node_checked("ws://a");
        assert!(!g.status(true, Some(h(4, "x4")), 0).ready);
        assert!(g.status(true, Some(h(5, "x5")), 0).ready);
    }

    #[test]
    fn local_differs_from_backup_blocks() {
        let mut g = Guard::new(false);
        g.reset_for_import(Some(h(2, "a")));
        g.add_node("ws://n");
        g.node_checked("ws://n");
        assert!(g.status(true, Some(h(2, "b")), 0).reason.unwrap().contains("fork"));
    }

    #[test]
    fn known_head_never_lowers() {
        let mut g = Guard::new(false);
        g.note_known(h(5, "x5"));
        g.note_known(h(3, "x3"));
        assert_eq!(g.known().unwrap().seq, 5);
    }

    #[test]
    fn node_added_after_import_must_be_checked() {
        let mut g = Guard::new(false);
        g.add_node("ws://a");
        g.reset_for_import(None);
        g.node_checked("ws://a");
        g.add_node("ws://b");
        assert!(!g.status(true, None, 0).ready);
    }

    #[test]
    fn fake_head_claim_blocks_only_until_timeout() {
        // A malicious node claims seq 1,000,000 for the user's log.
        let mut g = Guard::new(false);
        g.add_node("ws://evil.example/v1/sync");
        g.add_node("ws://good.example/v1/sync");
        g.reset_for_new_key();
        g.note_known(h(2, "x2"));
        g.node_reported("ws://evil.example/v1/sync", Some(h(1_000_000, "f")), 10_000);
        g.node_reported("ws://good.example/v1/sync", Some(h(2, "x2")), 10_000);
        let s = g.status(true, Some(h(2, "x2")), 10_000);
        assert!(!s.ready);
        assert!(s.reason.unwrap().contains("evil.example"));
        // The claim never becomes proven head: known stays at 2.
        assert_eq!(g.known().unwrap().seq, 2);
        // After the timeout the claim is ignored and named.
        let s = g.status(true, Some(h(2, "x2")), 10_000 + CLAIM_TIMEOUT_MS);
        assert!(s.ready, "{s:?}");
        let r = s.reason.unwrap();
        assert!(r.contains("evil.example") && r.contains("cannot prove"), "{r}");
        assert!(s.nodes.iter().any(|n| n.url.contains("evil") && n.unproven));
        assert!(s.nodes.iter().any(|n| n.url.contains("good") && !n.unproven));
    }

    #[test]
    fn fake_same_seq_claim_is_ignored_after_timeout() {
        let mut g = Guard::new(false);
        g.add_node("ws://n");
        g.reset_for_new_key();
        g.node_reported("ws://n", Some(h(1, "other")), 0);
        assert!(!g.status(true, Some(h(1, "mine")), 1).ready);
        assert!(g.status(true, Some(h(1, "mine")), CLAIM_TIMEOUT_MS).ready);
    }

    #[test]
    fn new_claim_restarts_the_timer() {
        let mut g = Guard::new(false);
        g.add_node("ws://n");
        g.reset_for_new_key();
        g.node_reported("ws://n", Some(h(5, "a")), 0);
        assert!(g.status(true, None, CLAIM_TIMEOUT_MS).ready);
        g.node_reported("ws://n", Some(h(6, "b")), CLAIM_TIMEOUT_MS + 1);
        assert!(!g.status(true, None, CLAIM_TIMEOUT_MS + 2).ready);
    }

    #[test]
    fn honest_lower_or_equal_claims_do_not_block() {
        let mut g = Guard::new(false);
        g.add_node("ws://n");
        g.reset_for_new_key();
        g.node_reported("ws://n", Some(h(1, "a")), 0);
        assert!(g.status(true, Some(h(3, "c")), 0).ready);
        g.node_reported("ws://n", Some(h(3, "c")), 0);
        let s = g.status(true, Some(h(3, "c")), 0);
        assert!(s.ready && s.reason.is_none());
    }
}
