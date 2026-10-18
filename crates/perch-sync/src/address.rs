//! Peer address table with caps per network group (docs/sync-protocol.md section 8).

use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, Ipv6Addr};

use crate::message::is_valid_address;

/// Defaults from docs/sync-protocol.md section 8.1.
pub const TABLE_SIZE: usize = 1000;
pub const PER_GROUP: usize = 16;
pub const SHARE_PER_GROUP: usize = 2;
pub const MAX_FAILURES: u32 = 3;

/// The host part of a `ws://` or `wss://` URL, lowercased, without brackets or port.
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("wss://").or_else(|| url.strip_prefix("ws://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next()?
    } else {
        authority.split(':').next()?
    };
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// The network group of an address: IPv4 /16, IPv6 /32, or the last two DNS labels.
pub fn group_of(url: &str) -> String {
    let Some(host) = host_of(url) else {
        return String::from("invalid");
    };
    if let Ok(v4) = host.parse::<Ipv4Addr>() {
        let o = v4.octets();
        return format!("v4:{}.{}", o[0], o[1]);
    }
    if let Ok(v6) = host.parse::<Ipv6Addr>() {
        let s = v6.segments();
        return format!("v6:{:x}:{:x}", s[0], s[1]);
    }
    let labels: Vec<&str> = host.trim_end_matches('.').split('.').collect();
    let n = labels.len();
    if n >= 2 {
        format!("dns:{}.{}", labels[n - 2], labels[n - 1])
    } else {
        format!("dns:{host}")
    }
}

/// True for an IPv4 address that is not publicly routable.
pub fn is_private_v4(v4: Ipv4Addr) -> bool {
    let o = v4.octets();
    v4.is_private()
        || v4.is_loopback()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_broadcast()
        || v4.is_multicast()
        || o[0] == 0
        || o[0] >= 240
        || (o[0] == 100 && (64..128).contains(&o[1]))
        || (o[0] == 192 && o[1] == 0 && o[2] == 0)
        || (o[0] == 198 && (18..20).contains(&o[1]))
}

/// True for an IP address that is not publicly routable. IPv4-mapped
/// (`::ffff:a.b.c.d`), IPv4-compatible (`::a.b.c.d`), and NAT64
/// (`64:ff9b::/96`) addresses are judged by their IPv4 part.
pub fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => is_private_v4(v4),
        std::net::IpAddr::V6(v6) => {
            let seg = v6.segments();
            let embedded = Ipv4Addr::new((seg[6] >> 8) as u8, seg[6] as u8, (seg[7] >> 8) as u8, seg[7] as u8);
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_v4(v4);
            }
            if seg[..6] == [0, 0, 0, 0, 0, 0] && !v6.is_loopback() && !v6.is_unspecified() {
                return is_private_v4(embedded);
            }
            if seg[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                return is_private_v4(embedded);
            }
            let s0 = seg[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s0 & 0xfe00) == 0xfc00
                || (s0 & 0xffc0) == 0xfe80
                || (s0 & 0xffc0) == 0xfec0
        }
    }
}

/// True for loopback, private, link-local, and other local-only hosts.
/// For DNS names this checks only the name; the dialer must also check the
/// resolved addresses.
pub fn is_private_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return is_private_ip(ip);
    }
    let h = host.trim_end_matches('.').to_ascii_lowercase();
    h == "localhost" || h.ends_with(".localhost") || h.ends_with(".local") || h.ends_with(".internal") || !h.contains('.')
}

/// True when the URL's host is private (see [`is_private_host`]).
pub fn is_private_address(url: &str) -> bool {
    host_of(url).map(|h| is_private_host(&h)).unwrap_or(true)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddressEntry {
    pub group: String,
    pub configured: bool,
    pub failures: u32,
    /// True if the address may resolve to a private IP: configured, or
    /// learned from a peer on a private network. Dialers check resolved
    /// addresses against this.
    pub private_ok: bool,
}

/// Known peer addresses. Configured addresses are never evicted.
#[derive(Clone, Debug)]
pub struct AddressBook {
    entries: BTreeMap<String, AddressEntry>,
    per_group: BTreeMap<String, usize>,
    pub capacity: usize,
    pub group_cap: usize,
}

impl Default for AddressBook {
    fn default() -> Self {
        AddressBook {
            entries: BTreeMap::new(),
            per_group: BTreeMap::new(),
            capacity: TABLE_SIZE,
            group_cap: PER_GROUP,
        }
    }
}

impl AddressBook {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, addr: &str) -> Option<&AddressEntry> {
        self.entries.get(addr)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &AddressEntry)> {
        self.entries.iter()
    }

    /// Adds an address from configuration. Always accepted if valid.
    pub fn add_configured(&mut self, addr: &str) -> bool {
        if !is_valid_address(addr) {
            return false;
        }
        if let Some(e) = self.entries.get_mut(addr) {
            e.configured = true;
            e.failures = 0;
            return true;
        }
        let group = group_of(addr);
        *self.per_group.entry(group.clone()).or_default() += 1;
        self.entries.insert(
            addr.to_string(),
            AddressEntry {
                group,
                configured: true,
                failures: 0,
                private_ok: true,
            },
        );
        true
    }

    /// Adds an address learned from a peer. `source_private` tells whether the
    /// peer that sent it is on a private network; public peers cannot add private addresses.
    pub fn add_learned(&mut self, addr: &str, source_private: bool) -> bool {
        if !is_valid_address(addr) || self.entries.contains_key(addr) {
            return false;
        }
        if !source_private && is_private_address(addr) {
            return false;
        }
        if self.entries.len() >= self.capacity {
            return false;
        }
        let group = group_of(addr);
        let n = self.per_group.get(&group).copied().unwrap_or(0);
        if n >= self.group_cap {
            return false;
        }
        self.per_group.insert(group.clone(), n + 1);
        self.entries.insert(
            addr.to_string(),
            AddressEntry {
                group,
                configured: false,
                failures: 0,
                private_ok: source_private,
            },
        );
        true
    }

    /// Records a failed dial. Learned addresses are removed after [`MAX_FAILURES`].
    pub fn record_failure(&mut self, addr: &str) {
        let remove = match self.entries.get_mut(addr) {
            Some(e) => {
                e.failures += 1;
                !e.configured && e.failures >= MAX_FAILURES
            }
            None => false,
        };
        if remove {
            self.remove(addr);
        }
    }

    pub fn record_success(&mut self, addr: &str) {
        if let Some(e) = self.entries.get_mut(addr) {
            e.failures = 0;
        }
    }

    pub fn remove(&mut self, addr: &str) {
        if let Some(e) = self.entries.remove(addr)
            && let Some(n) = self.per_group.get_mut(&e.group)
        {
            *n = n.saturating_sub(1);
        }
    }

    /// Up to `max` addresses to share with a peer: at most [`SHARE_PER_GROUP`]
    /// per group, excluding `exclude`, starting at a rotation given by `seed`.
    pub fn sample(&self, max: usize, seed: u64, exclude: &BTreeSet<String>) -> Vec<String> {
        let all: Vec<(&String, &AddressEntry)> =
            self.entries.iter().filter(|(a, _)| !exclude.contains(*a)).collect();
        if all.is_empty() {
            return Vec::new();
        }
        let start = (seed as usize) % all.len();
        let mut per_group: BTreeMap<&str, usize> = BTreeMap::new();
        let mut out = Vec::new();
        for i in 0..all.len() {
            if out.len() >= max {
                break;
            }
            let (addr, e) = all[(start + i) % all.len()];
            let n = per_group.entry(e.group.as_str()).or_default();
            if *n < SHARE_PER_GROUP {
                *n += 1;
                out.push(addr.clone());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_and_groups() {
        assert_eq!(host_of("ws://127.0.0.1:7701/v1/sync").as_deref(), Some("127.0.0.1"));
        assert_eq!(host_of("wss://[2001:db8::1]:443/v1/sync").as_deref(), Some("2001:db8::1"));
        assert_eq!(host_of("wss://Node.Example.ORG/v1/sync").as_deref(), Some("node.example.org"));
        assert_eq!(group_of("ws://198.51.100.7:7700/v1/sync"), "v4:198.51");
        assert_eq!(group_of("wss://[2001:db8:1:2::1]/v1/sync"), "v6:2001:db8");
        assert_eq!(group_of("wss://a.b.example.org/v1/sync"), "dns:example.org");
    }

    #[test]
    fn private_ranges() {
        for h in [
            "127.0.0.1", "10.1.2.3", "192.168.0.1", "172.16.5.4", "169.254.1.1", "100.64.0.1", "224.0.0.1", "0.1.2.3",
            "::1", "fe80::1", "fd00::1", "ff02::1", "::ffff:127.0.0.1", "::ffff:10.0.0.1", "::ffff:7f00:1", "::127.0.0.1",
            "64:ff9b::a00:1", "[::ffff:192.168.1.1]", "localhost", "LOCALHOST.", "printer.local", "intranet",
        ] {
            assert!(is_private_host(h), "{h}");
        }
        for h in ["198.51.100.7", "8.8.8.8", "::ffff:8.8.8.8", "2606:4700::1111", "perch.example.org"] {
            assert!(!is_private_host(h), "{h}");
        }
    }

    #[test]
    fn group_cap_and_private_filter() {
        let mut b = AddressBook::new();
        for i in 0..20 {
            b.add_learned(&format!("ws://198.51.{i}.1:7700/v1/sync"), false);
        }
        assert_eq!(b.len(), 16, "one /16 group capped at 16");
        assert!(!b.add_learned("ws://10.0.0.1:7700/v1/sync", false), "public peer cannot add private");
        assert!(b.add_learned("ws://10.0.0.1:7700/v1/sync", true));
        assert!(!b.add_learned("http://x.example.org/", true), "only ws urls");
        assert!(b.add_configured("ws://127.0.0.1:7701/v1/sync"));
    }

    #[test]
    fn failures_remove_learned_only() {
        let mut b = AddressBook::new();
        b.add_configured("ws://127.0.0.1:1/v1/sync");
        b.add_learned("ws://203.0.113.5:1/v1/sync", false);
        for _ in 0..MAX_FAILURES {
            b.record_failure("ws://127.0.0.1:1/v1/sync");
            b.record_failure("ws://203.0.113.5:1/v1/sync");
        }
        assert!(b.get("ws://127.0.0.1:1/v1/sync").is_some());
        assert!(b.get("ws://203.0.113.5:1/v1/sync").is_none());
    }

    #[test]
    fn sample_limits_per_group() {
        let mut b = AddressBook::new();
        for i in 0..10 {
            b.add_learned(&format!("ws://203.0.{i}.5:1/v1/sync"), false);
            b.add_learned(&format!("wss://n{i}.example.org/v1/sync"), false);
        }
        let s = b.sample(100, 3, &BTreeSet::new());
        assert_eq!(s.len(), 4, "two per group, two groups");
    }
}
