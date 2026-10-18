//! Settings of `perch-node serve`.

use std::net::SocketAddr;
use std::path::PathBuf;

/// Settings of a running node.
#[derive(Clone, Debug)]
pub struct ServeConfig {
    pub listen: SocketAddr,
    pub data: PathBuf,
    pub peers: Vec<String>,
    pub web: Option<PathBuf>,
    pub removed_files: Vec<PathBuf>,
    pub admin: Option<SocketAddr>,
    pub operator_key: Option<PathBuf>,
    /// URL other nodes dial to reach this node; announced in `hello.listen`.
    pub public_url: Option<String>,
    /// Admit every author in full (no tiers).
    pub open_admission: bool,
    /// Seconds between heads re-exchanges on each session.
    pub refresh_secs: u64,
    /// Maximum outbound connections, including configured peers.
    pub max_outbound: usize,
    /// JSONL files imported at startup.
    pub imports: Vec<PathBuf>,
}

/// Limits from ARCHITECTURE 9 and docs/sync-protocol.md 9.
pub mod limits {
    /// `POST /v1/records` body.
    pub const POST_BYTES: usize = 4 * 1024 * 1024;
    /// `POST /v1/records` records.
    pub const POST_RECORDS: usize = 1000;
    /// `GET /v1/authors/{id}/log` page.
    pub const LOG_PAGE_MAX: usize = 1000;
    pub const LOG_PAGE_DEFAULT: usize = 100;
    /// Inbound sync sessions.
    pub const INBOUND_SESSIONS: usize = 256;
    pub const INBOUND_PER_IP: usize = 8;
    /// Inbound sync messages per session per second, and burst.
    pub const MSG_RATE: f64 = 100.0;
    pub const MSG_BURST: f64 = 200.0;
    /// Inbound sync bytes per session per second, and burst.
    pub const BYTE_RATE: f64 = 16.0 * 1024.0 * 1024.0;
    pub const BYTE_BURST: f64 = 32.0 * 1024.0 * 1024.0;
    /// `POST /v1/records` per client IP per second, and burst.
    pub const POST_RATE: f64 = 2.0;
    pub const POST_BURST: f64 = 10.0;
    /// Seconds above a rate limit before a session is closed.
    pub const LIMITED_CLOSE_SECS: u64 = 10;
    /// Keepalive.
    pub const PING_SECS: u64 = 30;
    pub const IDLE_CLOSE_SECS: u64 = 90;
    /// Lookup prefix length range (hex characters).
    pub const LOOKUP_MIN: usize = 3;
    pub const LOOKUP_MAX: usize = 64;
}
