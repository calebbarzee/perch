//! Outbound connections: configured peers with reconnect and backoff, plus
//! learned addresses that fill free outbound slots (docs/sync-protocol.md 8).

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::node::{Node, now_ms};
use crate::ws;

/// Backoff with ±20% jitter: 1 s doubling to 60 s.
pub struct Backoff {
    next: Duration,
    seed: u64,
}

impl Backoff {
    pub fn new(seed: u64) -> Backoff {
        Backoff { next: Duration::from_secs(1), seed: seed | 1 }
    }

    pub fn reset(&mut self) {
        self.next = Duration::from_secs(1);
    }

    /// The next delay.
    pub fn next_delay(&mut self) -> Duration {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        let jitter = (self.seed % 401) as f64 / 1000.0 - 0.2;
        let d = self.next.mul_f64(1.0 + jitter);
        self.next = (self.next * 2).min(Duration::from_secs(60));
        d
    }
}

fn seed_for(url: &str) -> u64 {
    let h = perch_core::sha256(format!("{url}{}", now_ms()).as_bytes());
    u64::from_le_bytes([h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]])
}

/// Keeps a configured peer connected for the life of the process.
pub async fn keep_connected(node: Arc<Node>, url: String) {
    let mut backoff = Backoff::new(seed_for(&url));
    loop {
        let started = Instant::now();
        match ws::dial(node.clone(), &url, true).await {
            Ok(end) => {
                node.addresses.lock().unwrap_or_else(|e| e.into_inner()).record_success(&url);
                tracing::info!(url = %url, reason = %end.reason, "outbound session ended");
                if end.self_connection {
                    tracing::warn!(url = %url, "peer is this node; not reconnecting");
                    node.addresses.lock().unwrap_or_else(|e| e.into_inner()).remove(&url);
                    return;
                }
                if end.reached_live || started.elapsed() > Duration::from_secs(60) {
                    backoff.reset();
                }
            }
            Err(e) => {
                tracing::debug!(url = %url, error = %e, "connect failed");
                node.addresses.lock().unwrap_or_else(|e| e.into_inner()).record_failure(&url);
            }
        }
        tokio::time::sleep(backoff.next_delay()).await;
    }
}

/// Every 30 s, dials learned addresses while outbound connections are below the limit.
pub async fn fill_slots(node: Arc<Node>) {
    let configured: BTreeSet<String> = node.config.peers.iter().cloned().collect();
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;
        let (outbound, connected): (usize, BTreeSet<String>) = {
            let peers = node.peers.lock().unwrap_or_else(|e| e.into_inner());
            let out = peers.values().filter(|p| p.outbound).count();
            let urls = peers.values().filter_map(|p| p.url.clone()).collect();
            (out, urls)
        };
        let free = node.config.max_outbound.saturating_sub(outbound.max(configured.len()));
        if free == 0 {
            continue;
        }
        let candidates: Vec<(String, bool)> = {
            let book = node.addresses.lock().unwrap_or_else(|e| e.into_inner());
            book.iter()
                .filter(|(a, e)| !e.configured && !connected.contains(*a) && Some(*a) != node.config.public_url.as_ref())
                .map(|(a, e)| (a.clone(), e.private_ok))
                .take(free)
                .collect()
        };
        for (url, private_ok) in candidates {
            let node = node.clone();
            tokio::spawn(async move {
                match ws::dial(node.clone(), &url, private_ok).await {
                    Ok(end) => {
                        let mut book = node.addresses.lock().unwrap_or_else(|e| e.into_inner());
                        if end.self_connection {
                            book.remove(&url);
                        } else {
                            book.record_success(&url);
                        }
                    }
                    Err(_) => node.addresses.lock().unwrap_or_else(|e| e.into_inner()).record_failure(&url),
                }
            });
        }
    }
}
