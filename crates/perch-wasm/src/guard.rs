//! Publish guard (ARCHITECTURE 8.2, 10).
//!
//! A client must never sign a record at or below a position it has already
//! used, or it forks its own log. The guard tracks the highest own head known
//! from every source and refuses to publish until:
//!
//! 1. the local store holds the author's log up to that head, and
//! 2. with an imported key, every configured node has been synced since the
//!    import (at least 2 when 2 or more are configured).
//!
//! Pure logic, no I/O; tested natively.

use std::collections::BTreeMap;

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
    /// The node's head for the author, once its heads were checked.
    pub head: Option<HeadRef>,
    /// True once the node's heads were compared after the key was imported.
    pub checked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishStatus {
    pub ready: bool,
    pub reason: Option<String>,
    pub local_head: Option<HeadRef>,
    pub nodes: Vec<NodeHead>,
}

#[derive(Clone, Debug, Default)]
pub struct Guard {
    /// Highest head from a backup or from device storage.
    known: Option<HeadRef>,
    /// The key was imported (or restored): another device may have published.
    imported: bool,
    /// Per configured node: head and whether it was checked since the import.
    nodes: BTreeMap<String, NodeHead>,
    sparse: bool,
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
        }
    }

    /// An imported key, with the head from its backup if any.
    pub fn reset_for_import(&mut self, backup_head: Option<HeadRef>) {
        self.known = backup_head;
        self.imported = true;
        for n in self.nodes.values_mut() {
            n.head = None;
            n.checked = false;
        }
    }

    /// Raise the known head (never lowers it).
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
        self.nodes.entry(url.to_string()).or_insert_with(|| NodeHead { url: url.to_string(), head: None, checked: !imported });
    }

    pub fn remove_node(&mut self, url: &str) {
        self.nodes.remove(url);
    }

    /// A node's heads page covered the author: record its head (seq 0 = none).
    pub fn node_reported(&mut self, url: &str, head: Option<HeadRef>) {
        if let Some(h) = head.clone() {
            self.note_known(h);
        }
        if let Some(n) = self.nodes.get_mut(url) {
            if let Some(h) = head {
                if n.head.as_ref().is_none_or(|x| h.seq > x.seq) {
                    n.head = Some(h);
                }
            }
        }
    }

    /// The node finished its initial heads exchange after the import.
    pub fn node_checked(&mut self, url: &str) {
        if let Some(n) = self.nodes.get_mut(url) {
            n.checked = true;
        }
    }

    /// Computes whether publishing is allowed, given the local store's head.
    pub fn status(&self, has_key: bool, local_head: Option<HeadRef>) -> PublishStatus {
        let nodes: Vec<NodeHead> = self.nodes.values().cloned().collect();
        let reason = self.blocker(has_key, local_head.as_ref());
        PublishStatus { ready: reason.is_none(), reason, local_head, nodes }
    }

    fn blocker(&self, has_key: bool, local: Option<&HeadRef>) -> Option<String> {
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
                return Some(format!(
                    "This key was imported. Waiting for sync from {} of {} nodes.",
                    total - checked,
                    total
                ));
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
                    "Record {} of your log on this device differs from the one known elsewhere. Publishing now would fork your log.",
                    k.seq
                ));
            }
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
        assert!(g.status(true, None).ready);
    }

    #[test]
    fn no_key_blocks() {
        let g = Guard::new(false);
        assert!(!g.status(false, None).ready);
    }

    #[test]
    fn sparse_blocks() {
        let mut g = Guard::new(true);
        g.reset_for_new_key();
        assert!(g.status(true, None).reason.unwrap().contains("sparse"));
    }

    #[test]
    fn import_waits_for_every_node() {
        let mut g = Guard::new(false);
        g.add_node("ws://a");
        g.add_node("ws://b");
        g.reset_for_import(None);
        let s = g.status(true, None);
        assert!(!s.ready);
        assert!(s.reason.unwrap().contains("2 of 2"));
        g.node_reported("ws://a", Some(h(3, "x3")));
        g.node_checked("ws://a");
        assert!(g.status(true, None).reason.unwrap().contains("1 of 2"));
        g.node_reported("ws://b", None);
        g.node_checked("ws://b");
        // Node a has 3 records the device lacks.
        let s = g.status(true, None);
        assert!(s.reason.unwrap().contains("has 3 records"));
        assert!(g.status(true, Some(h(3, "x3"))).ready);
    }

    #[test]
    fn import_without_nodes_blocks() {
        let mut g = Guard::new(false);
        g.reset_for_import(None);
        assert!(g.status(true, None).reason.unwrap().contains("Connect to a node"));
    }

    #[test]
    fn backup_head_above_local_blocks() {
        let mut g = Guard::new(false);
        g.add_node("ws://a");
        g.reset_for_import(Some(h(5, "x5")));
        g.node_reported("ws://a", Some(h(4, "x4")));
        g.node_checked("ws://a");
        // The node is one behind the backup: the guard keeps the higher head.
        assert!(!g.status(true, Some(h(4, "x4"))).ready);
        assert!(g.status(true, Some(h(5, "x5"))).ready);
    }

    #[test]
    fn same_seq_different_id_blocks() {
        let mut g = Guard::new(false);
        g.reset_for_new_key();
        g.note_known(h(2, "a"));
        assert!(g.status(true, Some(h(2, "b"))).reason.unwrap().contains("fork"));
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
        assert!(!g.status(true, None).ready);
    }
}
