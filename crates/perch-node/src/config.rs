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
    /// Admission tier limits.
    pub tiers: crate::admission::TierLimits,
    /// Header holding the client address when the node runs behind a
    /// reverse proxy on the same host (for example `x-forwarded-for`).
    /// Used only for connections from loopback.
    pub trust_proxy_header: Option<String>,
    /// Which value of the trusted header is the client, counted from the
    /// right: 1 = the value the nearest proxy appended.
    pub trust_proxy_hops: usize,
    /// Extra trust roots (PEM) for `wss://` peers.
    pub ca_file: Option<PathBuf>,
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
    /// Heavy GET endpoints (views, related sets, lookup, export) per client
    /// IP per second, and burst. Export costs `EXPORT_COST` tokens.
    pub const HEAVY_RATE: f64 = 20.0;
    pub const HEAVY_BURST: f64 = 40.0;
    pub const EXPORT_COST: f64 = 5.0;
    /// Concurrent view computations; more wait up to `VIEW_WAIT_SECS`, then get 503.
    pub const VIEW_WORKERS: usize = 4;
    pub const VIEW_WAIT_SECS: u64 = 10;
    /// Heavy requests in flight per client IP; more wait up to `VIEW_WAIT_SECS`, then get 429.
    pub const PER_IP_IN_FLIGHT: usize = 1;
    /// Time budget of one lookup; checked between products.
    pub const LOOKUP_BUDGET_MS: u64 = 5_000;
    /// Rate-limiter table size before idle entries are evicted.
    pub const LIMITER_ENTRIES: usize = 100_000;
    /// Lookup prefix length range (hex characters).
    pub const LOOKUP_MIN: usize = 4;
    pub const LOOKUP_MAX: usize = 64;
}
