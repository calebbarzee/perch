//! The HTTP API (ARCHITECTURE 8.1, docs/api.md).

use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{ConnectInfo, Path, RawQuery, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use perch_core::{AuthorId, Record, RecordId};
use perch_store::{Insert, Store};
use perch_views::{SearchOptions, Sort, Views, canonical_json};
use serde_json::{Value, json};
use tower_http::cors::{Any, CorsLayer};

use crate::config::limits;
use crate::node::{Node, NodeError, now_ms};

/// Header carrying the policy hash on view responses.
pub const POLICY_HASH_HEADER: &str = "x-perch-policy-hash";
/// Header set on `/v1/lookup` when the time budget ended the response early:
/// `<products included>/<products in the bucket>`.
pub const TRUNCATED_HEADER: &str = "x-perch-truncated";

/// The key used for per-client limits: IPv6 addresses count per /64 (one
/// subscriber usually holds a whole /64); IPv4-mapped IPv6 counts as IPv4.
pub fn limit_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return IpAddr::V4(v4);
            }
            let s = v6.segments();
            IpAddr::V6(std::net::Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
        }
    }
}

/// Per-client-IP token buckets. When the table is full, idle entries (whose
/// bucket has refilled) are evicted first, then the entries with the most
/// tokens. Clients that are being limited keep their state (audit #14).
pub struct Limiter {
    rate: f64,
    burst: f64,
    buckets: Mutex<HashMap<IpAddr, (f64, Instant)>>,
}

impl Limiter {
    pub fn new(rate: f64, burst: f64) -> Limiter {
        Limiter { rate, burst, buckets: Mutex::new(HashMap::new()) }
    }

    pub fn take(&self, ip: IpAddr, cost: f64) -> bool {
        let mut m = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        if m.len() >= limits::LIMITER_ENTRIES && !m.contains_key(&ip) {
            let full_after = self.burst / self.rate;
            m.retain(|_, (_, at)| now.duration_since(*at).as_secs_f64() < full_after);
            if m.len() >= limits::LIMITER_ENTRIES {
                let (rate, burst) = (self.rate, self.burst);
                let mut levels: Vec<(u64, IpAddr)> = m
                    .iter()
                    .map(|(k, (lvl, at))| {
                        let now_lvl = (lvl + now.duration_since(*at).as_secs_f64() * rate).min(burst);
                        ((now_lvl * 1000.0) as u64, *k)
                    })
                    .collect();
                levels.sort_by(|a, b| b.cmp(a));
                for (_, k) in levels.iter().take(m.len() / 10 + 1) {
                    m.remove(k);
                }
            }
        }
        let e = m.entry(ip).or_insert((self.burst, now));
        e.0 = (e.0 + now.duration_since(e.1).as_secs_f64() * self.rate).min(self.burst);
        e.1 = now;
        if e.0 >= cost {
            e.0 -= cost;
            true
        } else {
            false
        }
    }

    /// Charges `cost` tokens after the fact (for example for time used); the
    /// bucket may go into debt down to minus one burst.
    pub fn charge(&self, ip: IpAddr, cost: f64) {
        let mut m = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = m.get_mut(&ip) {
            e.0 = (e.0 - cost).max(-self.burst);
        }
    }

    pub fn len(&self) -> usize {
        self.buckets.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod limiter_tests {
    use super::*;

    #[test]
    fn ipv6_clients_count_per_64() {
        let a: IpAddr = "2001:db8:1:2:aaaa::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:bbbb::9".parse().unwrap();
        let c: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert_eq!(limit_key(a), limit_key(b));
        assert_ne!(limit_key(a), limit_key(c));
        assert_eq!(limit_key("::ffff:203.0.113.5".parse().unwrap()), "203.0.113.5".parse::<IpAddr>().unwrap());
        let l = Limiter::new(1.0, 2.0);
        assert!(l.take(limit_key(a), 2.0));
        assert!(!l.take(limit_key(b), 1.0), "same /64 shares the bucket");
        assert!(l.take(limit_key(c), 1.0));
    }

    #[test]
    fn forwarded_ip_takes_the_trusted_hop() {
        // The client wrote "1.1.1.1"; the proxy appended the real peer.
        assert_eq!(forwarded_ip("1.1.1.1, 203.0.113.7", 1), Some("203.0.113.7".parse().unwrap()));
        assert_eq!(forwarded_ip("1.1.1.1, 203.0.113.7, 10.0.0.2", 2), Some("203.0.113.7".parse().unwrap()));
        assert_eq!(forwarded_ip("203.0.113.7", 3), Some("203.0.113.7".parse().unwrap()));
        assert_eq!(forwarded_ip("[2001:db8::1]:443", 1), Some("2001:db8::1".parse().unwrap()));
        assert_eq!(forwarded_ip("", 1), None);
        assert_eq!(forwarded_ip("junk", 1), None);
    }

    #[tokio::test]
    async fn one_heavy_request_in_flight_per_address() {
        let f = InFlight::default();
        let a: IpAddr = "203.0.113.1".parse().unwrap();
        let b: IpAddr = "203.0.113.2".parse().unwrap();
        let wait = std::time::Duration::from_millis(100);
        let held = f.acquire(a, wait).await.expect("first slot");
        assert!(f.acquire(a, wait).await.is_none(), "second request from the same address waits, then fails");
        assert!(f.acquire(b, wait).await.is_some(), "other addresses are not blocked");
        drop(held);
        assert!(f.acquire(a, wait).await.is_some(), "slot released");
    }

    #[test]
    fn slow_requests_are_charged() {
        let l = Limiter::new(20.0, 40.0);
        let ip: IpAddr = "203.0.113.3".parse().unwrap();
        assert!(l.take(ip, 1.0));
        l.charge(ip, 100.0);
        assert!(!l.take(ip, 1.0), "debt blocks further requests");
    }

    #[test]
    fn evicts_idle_entries_instead_of_resetting_all() {
        let l = Limiter::new(1.0, 2.0);
        let hot: IpAddr = "203.0.113.9".parse().unwrap();
        assert!(l.take(hot, 1.0) && l.take(hot, 1.0));
        assert!(!l.take(hot, 1.0), "bucket empty");
        for i in 0..limits::LIMITER_ENTRIES as u32 {
            l.take(IpAddr::from(std::net::Ipv4Addr::from(0x0a00_0000 + i)), 0.0);
        }
        assert!(l.len() <= limits::LIMITER_ENTRIES + 1);
        assert!(!l.take(hot, 1.0), "a full table does not refill an active client");
    }
}

/// The client address in a proxy header: the `hops`-th value from the right
/// (1 = the value the nearest trusted proxy appended). Values to its left
/// come from the client and are not trusted. With fewer values, the leftmost.
pub fn forwarded_ip(value: &str, hops: usize) -> Option<IpAddr> {
    let parts: Vec<&str> = value.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    let i = parts.len().saturating_sub(hops.max(1));
    let p = parts[i];
    let p = p.strip_prefix('[').and_then(|x| x.split(']').next()).unwrap_or(p);
    p.parse::<IpAddr>().ok().or_else(|| p.parse::<SocketAddr>().ok().map(|s| s.ip()))
}

/// Per-client-IP limit on heavy requests in flight (audit #10).
#[derive(Default)]
pub struct InFlight {
    per_ip: Mutex<HashMap<IpAddr, Arc<tokio::sync::Semaphore>>>,
}

impl InFlight {
    /// Waits up to `wait` for one of the IP's slots.
    pub async fn acquire(&self, ip: IpAddr, wait: std::time::Duration) -> Option<tokio::sync::OwnedSemaphorePermit> {
        let sem = {
            let mut m = self.per_ip.lock().unwrap_or_else(|e| e.into_inner());
            if m.len() >= limits::LIMITER_ENTRIES {
                // Drop entries nobody holds or waits on.
                m.retain(|_, s| Arc::strong_count(s) > 1);
            }
            m.entry(ip).or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(limits::PER_IP_IN_FLIGHT))).clone()
        };
        tokio::time::timeout(wait, sem.acquire_owned()).await.ok()?.ok()
    }
}

/// Held while a heavy request runs. On drop, charges the client one rate
/// token per 50 ms of time used, so slow queries cost more.
pub struct HeavyGuard {
    limiter: Arc<Limiter>,
    ip: IpAddr,
    start: Instant,
    _slot: tokio::sync::OwnedSemaphorePermit,
}

impl Drop for HeavyGuard {
    fn drop(&mut self) {
        let extra = self.start.elapsed().as_secs_f64() * limits::HEAVY_RATE;
        if extra >= 1.0 {
            self.limiter.charge(self.ip, extra - 1.0);
        }
    }
}

/// Shared state of the HTTP layer.
#[derive(Clone)]
pub struct Http {
    pub node: Arc<Node>,
    post_limit: Arc<Limiter>,
    heavy_limit: Arc<Limiter>,
    views: Arc<tokio::sync::Semaphore>,
    in_flight: Arc<InFlight>,
}

/// An API error: `{ "error": { "code", "message" } }`.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> ApiError {
        ApiError { status, code, message: message.into() }
    }
    fn bad(code: &'static str, m: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, code, m)
    }
    fn not_found(m: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::NOT_FOUND, "not_found", m)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({ "error": { "code": self.code, "message": self.message } }).to_string();
        (self.status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
    }
}

impl From<NodeError> for ApiError {
    fn from(e: NodeError) -> Self {
        match e {
            NodeError::BadRequest(m) => ApiError::bad("bad_request", m),
            NodeError::Conflict(m) => ApiError::new(StatusCode::CONFLICT, "conflict", m),
            NodeError::NotFound(m) => ApiError::not_found(m),
            NodeError::Internal(m) => ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", m),
        }
    }
}

type ApiResult = Result<Response, ApiError>;

fn query(raw: &Option<String>) -> BTreeMap<String, String> {
    match raw {
        Some(q) => form_urlencoded::parse(q.as_bytes()).into_owned().collect(),
        None => BTreeMap::new(),
    }
}

fn q_u64(q: &BTreeMap<String, String>, name: &str) -> Result<Option<u64>, ApiError> {
    match q.get(name) {
        None => Ok(None),
        Some(v) => match v.parse::<u64>() {
            Ok(n) if n <= perch_core::limits::MAX_SAFE_INTEGER => Ok(Some(n)),
            _ => Err(ApiError::bad("bad_query", format!("{name} must be an integer from 0 to 2^53 - 1"))),
        },
    }
}

fn parse_id(s: &str) -> Result<RecordId, ApiError> {
    RecordId::from_hex(s).map_err(|e| ApiError::bad("bad_id", format!("record ID: {e}")))
}

fn parse_author(s: &str) -> Result<AuthorId, ApiError> {
    AuthorId::from_hex(s).map_err(|e| ApiError::bad("bad_id", format!("author ID: {e}")))
}

fn json_response(status: StatusCode, body: String) -> Response {
    (status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
}

fn with_policy_hash(mut r: Response, hash: &str) -> Response {
    if let Ok(v) = HeaderValue::from_str(hash) {
        r.headers_mut().insert(POLICY_HASH_HEADER, v);
    }
    r
}

fn view_text(node: &Node, body: String) -> Response {
    with_policy_hash(json_response(StatusCode::OK, body), &node.policy_hash)
}

fn jsonl_response(lines: Vec<String>, hash: Option<&str>) -> Response {
    let mut body = String::with_capacity(lines.iter().map(|l| l.len() + 1).sum());
    for l in lines {
        body.push_str(&l);
        body.push('\n');
    }
    let r = (StatusCode::OK, [(header::CONTENT_TYPE, "application/x-ndjson")], body).into_response();
    match hash {
        Some(h) => with_policy_hash(r, h),
        None => r,
    }
}

/// Builds the router for the public API.
pub fn router(node: Arc<Node>) -> Router {
    let state = Http {
        node: node.clone(),
        post_limit: Arc::new(Limiter::new(limits::POST_RATE, limits::POST_BURST)),
        heavy_limit: Arc::new(Limiter::new(limits::HEAVY_RATE, limits::HEAVY_BURST)),
        views: Arc::new(tokio::sync::Semaphore::new(limits::VIEW_WORKERS)),
        in_flight: Arc::new(InFlight::default()),
    };
    let api = Router::new()
        .route("/v1/status", get(status))
        .route("/v1/policy", get(policy))
        .route("/v1/records", axum::routing::post(post_records))
        .route("/v1/records/{id}", get(get_record))
        .route("/v1/authors/{id}", get(get_author))
        .route("/v1/authors/{id}/log", get(get_log))
        .route("/v1/search", get(search))
        .route("/v1/products/{key}", get(get_product))
        .route("/v1/products/{key}/records", get(get_product_records))
        .route("/v1/lookup/{prefix}", get(lookup))
        .route("/v1/listings/{id}", get(get_listing))
        .route("/v1/stats", get(stats))
        .route("/v1/export.jsonl", get(export))
        .route("/v1/sync", get(sync_ws))
        .route("/v1/{*rest}", get(unknown_api))
        .with_state(state);
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::HEAD, Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE])
        .expose_headers([header::HeaderName::from_static(POLICY_HASH_HEADER), header::HeaderName::from_static(TRUNCATED_HEADER)]);
    let app = match &node.config.web {
        Some(dir) => api.fallback_service(tower_http::services::ServeDir::new(dir).append_index_html_on_directories(true)),
        None => api.fallback(|| async { ApiError::not_found("no such path; this node does not serve a web app (--web)") }),
    };
    app.layer(cors)
}

async fn unknown_api() -> ApiError {
    ApiError::not_found("no such API endpoint; see docs/api.md")
}

async fn status(State(h): State<Http>) -> Response {
    let n = &h.node;
    let store = {
        let st = n.read();
        json!({
            "records": st.len(),
            "authors": st.author_count(),
            "forked_authors": st.forked_authors().len(),
            "products": st.product_key_count(),
            "held": st.held_count(),
            "bodyless": st.bodyless_count(),
            "removed": st.removed_count(),
            "tier3_pool_bytes": st.pool_bytes(),
        })
    };
    let peers: Vec<Value> = n.peers.lock().unwrap_or_else(|e| e.into_inner()).values().map(|p| p.to_json()).collect();
    let known = n.addresses.lock().unwrap_or_else(|e| e.into_inner()).len();
    let body = json!({
        "node": n.node_id,
        "software": format!("perch-node/{}", n.build.version),
        "version": n.build.version,
        "build_hash": n.build.build_hash,
        "protocol": perch_sync::VERSION,
        "listen": n.config.listen.to_string(),
        "public_url": n.config.public_url,
        "started_at": n.started_ms,
        "uptime_ms": n.started.elapsed().as_millis() as u64,
        "now": now_ms(),
        "policy_hash": n.policy_hash,
        "admission": if n.admission.is_some() { "tiers" } else { "open" },
        "store": store,
        "peers": peers,
        "known_addresses": known,
    });
    json_response(StatusCode::OK, body.to_string())
}

async fn policy(State(h): State<Http>, RawQuery(raw): RawQuery) -> ApiResult {
    let q = query(&raw);
    let p = h.node.policy_at(q_u64(&q, "now")?);
    let body = json!({ "policy": p.to_value(), "hash": h.node.policy_hash });
    Ok(with_policy_hash(json_response(StatusCode::OK, body.to_string()), &h.node.policy_hash))
}

impl Http {
    /// The client address: the socket peer, or the trusted proxy header for
    /// connections from loopback when `--trust-proxy-header` is set.
    pub fn client_ip(&self, addr: SocketAddr, headers: &HeaderMap) -> IpAddr {
        if addr.ip().is_loopback()
            && let Some(h) = &self.node.config.trust_proxy_header
        {
            let joined: Vec<&str> = headers.get_all(h.as_str()).iter().filter_map(|v| v.to_str().ok()).collect();
            if let Some(ip) = forwarded_ip(&joined.join(","), self.node.config.trust_proxy_hops) {
                return ip;
            }
        }
        addr.ip()
    }

    /// Rate limit, then one in-flight slot per client (IPv6: per /64) (audit #10).
    async fn heavy(&self, addr: SocketAddr, headers: &HeaderMap, cost: f64) -> Result<HeavyGuard, ApiError> {
        let ip = limit_key(self.client_ip(addr, headers));
        if !self.heavy_limit.take(ip, cost) {
            return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "too many view requests from this address; retry later"));
        }
        let slot = self
            .in_flight
            .acquire(ip, std::time::Duration::from_secs(limits::VIEW_WAIT_SECS))
            .await
            .ok_or_else(|| {
                ApiError::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "one view request at a time per address; retry later")
            })?;
        Ok(HeavyGuard { limiter: self.heavy_limit.clone(), ip, start: Instant::now(), _slot: slot })
    }

    /// Runs `f` on the blocking pool, at most `VIEW_WORKERS` at a time (audit #10).
    async fn compute<T: Send + 'static>(&self, f: impl FnOnce(&Node) -> T + Send + 'static) -> Result<T, ApiError> {
        let permit = tokio::time::timeout(
            std::time::Duration::from_secs(limits::VIEW_WAIT_SECS),
            self.views.clone().acquire_owned(),
        )
        .await
        .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "busy", "the node is busy; retry later"))?
        .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "busy", "the node is stopping"))?;
        let node = self.node.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            f(&node)
        })
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string()))
    }
}

async fn post_records(State(h): State<Http>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, body: Body) -> ApiResult {
    if !h.post_limit.take(limit_key(h.client_ip(addr, &headers)), 1.0) {
        return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "at most 2 submissions per second per address"));
    }
    let bytes = axum::body::to_bytes(body, limits::POST_BYTES).await.map_err(|_| {
        ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large", format!("body larger than {} bytes", limits::POST_BYTES))
    })?;
    let text = std::str::from_utf8(&bytes).map_err(|_| ApiError::bad("bad_encoding", "body is not UTF-8"))?;
    // (input position, parse result)
    let mut items: Vec<(usize, Result<Record, String>)> = Vec::new();
    let trimmed = text.trim_start();
    if trimmed.starts_with('[') {
        // Each element: 131,072-byte limit on its exact text, then strict parsing (audit #12).
        let arr = perch_store::parse_record_array(trimmed).map_err(|e| ApiError::bad("bad_json", e))?;
        if arr.len() > limits::POST_RECORDS {
            return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too_many_records", format!("{} records; limit {}", arr.len(), limits::POST_RECORDS)));
        }
        items.extend(arr.into_iter().enumerate());
    } else {
        for (line, r) in perch_core::parse_jsonl(text) {
            if items.len() >= limits::POST_RECORDS {
                return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too_many_records", format!("more than {} records", limits::POST_RECORDS)));
            }
            items.push((line, r.map_err(|e| e.to_string())));
        }
    }
    let valid: Vec<Record> = items.iter().filter_map(|(_, r)| r.as_ref().ok().cloned()).collect();
    let outcomes = h.compute(move |n| n.submit(valid)).await??;
    let mut outs = outcomes.into_iter();
    let key = if trimmed.starts_with('[') { "index" } else { "line" };
    let mut summary: BTreeMap<&str, usize> = ["inserted", "duplicate", "fork", "gap", "rejected"].iter().map(|k| (*k, 0)).collect();
    let mut results = Vec::with_capacity(items.len());
    for (pos, r) in items {
        let entry = match r {
            Err(e) => {
                *summary.entry("rejected").or_default() += 1;
                json!({ key: pos, "id": null, "outcome": "invalid", "error": e })
            }
            Ok(rec) => {
                let o = outs.next().unwrap_or(Insert::Duplicate);
                let bucket = match &o {
                    Insert::Inserted | Insert::BodyFilled => "inserted",
                    Insert::Duplicate => "duplicate",
                    Insert::Fork { .. } => "fork",
                    Insert::Gap | Insert::AboveFork => "gap",
                    Insert::Rejected(_) => "rejected",
                };
                *summary.entry(bucket).or_default() += 1;
                let error = match &o {
                    Insert::Rejected(r) => Some(r.to_string()),
                    _ => None,
                };
                json!({ key: pos, "id": rec.id().to_hex(), "outcome": o.name(), "error": error })
            }
        };
        results.push(entry);
    }
    let body = json!({ "summary": summary, "results": results });
    Ok(json_response(StatusCode::OK, body.to_string()))
}

async fn get_record(State(h): State<Http>, Path(id): Path<String>) -> ApiResult {
    let id = parse_id(&id)?;
    let r = h.node.read().get(&id).ok_or_else(|| ApiError::not_found(format!("record {id} is not stored on this node")))?;
    Ok(json_response(StatusCode::OK, r.to_json_string()))
}

async fn get_author(
    State(h): State<Http>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> ApiResult {
    let a = parse_author(&id)?;
    let p = h.node.policy_at(q_u64(&query(&raw), "now")?);
    let _g = h.heavy(addr, &headers, 1.0).await?;
    let v = h.compute(move |n| perch_views::author_view(&*n.read(), &p, &a).map(|v| canonical_json(&v))).await?;
    let v = v.ok_or_else(|| ApiError::not_found(format!("author {a} is not known to this node")))?;
    Ok(view_text(&h.node, v))
}

async fn get_log(State(h): State<Http>, Path(id): Path<String>, RawQuery(raw): RawQuery) -> ApiResult {
    let a = parse_author(&id)?;
    let q = query(&raw);
    let from = q_u64(&q, "from")?.unwrap_or(1).max(1);
    let limit = q_u64(&q, "limit")?.map_or(limits::LOG_PAGE_DEFAULT, |l| (l as usize).clamp(1, limits::LOG_PAGE_MAX));
    let recs = h.node.read().log(&a, from, limit);
    let mut body = String::from("[");
    for (i, r) in recs.iter().enumerate() {
        if i > 0 {
            body.push(',');
        }
        body.push_str(&r.to_json_string());
    }
    body.push(']');
    Ok(json_response(StatusCode::OK, body))
}

async fn search(
    State(h): State<Http>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> ApiResult {
    let q = query(&raw);
    let p = h.node.policy_at(q_u64(&q, "now")?);
    let mut opts = SearchOptions::default();
    if let Some(s) = q.get("sort") {
        opts.sort = Sort::parse(s).ok_or_else(|| ApiError::bad("bad_query", "sort must be relevance, price, rating_count, or rating_mean"))?;
    }
    if let Some(c) = q.get("currency") {
        opts.currency = Some(c.clone());
    }
    if let Some(l) = q_u64(&q, "limit")? {
        opts.limit = (l as usize).clamp(1, perch_views::MAX_LIMIT);
    }
    let text = q.get("q").cloned().unwrap_or_default();
    let _g = h.heavy(addr, &headers, 1.0).await?;
    let hits = h.compute(move |n| canonical_json(&perch_views::search(&*n.read(), &p, &text, &opts))).await?;
    Ok(view_text(&h.node, hits))
}

async fn get_product(
    State(h): State<Http>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(key): Path<String>,
    RawQuery(raw): RawQuery,
) -> ApiResult {
    let p = h.node.policy_at(q_u64(&query(&raw), "now")?);
    let _g = h.heavy(addr, &headers, 1.0).await?;
    let k = key.clone();
    let v = h.compute(move |n| perch_views::product_view(&*n.read(), &p, &k).map(|v| canonical_json(&v))).await?;
    let v = v.ok_or_else(|| ApiError::not_found(format!("product {key:?} is not known to this node")))?;
    Ok(view_text(&h.node, v))
}

/// Related sets of `keys` as JSONL lines, in key order, until `deadline`.
/// Returns the lines and the number of keys included.
fn related_lines(node: &Node, keys: &[String], now: Option<u64>, deadline: Option<Instant>) -> (Vec<String>, usize) {
    let p = node.policy_at(now);
    let st = node.read();
    let mut ids = std::collections::BTreeSet::new();
    let mut done = 0;
    for k in keys {
        // At least one product is always included, so a lookup makes progress.
        if done > 0 && deadline.is_some_and(|d| Instant::now() > d) {
            break;
        }
        ids.extend(perch_views::related_records(&*st, k, &p));
        done += 1;
    }
    (ids.into_iter().filter_map(|id| st.get(&id)).map(|r| r.to_json_string()).collect(), done)
}

/// Orders a lookup bucket: keys with a visible product chain by a verified
/// author first, then the rest, each in key order. Fake products made with
/// throwaway keys therefore cannot push a verified product past the budget.
fn lookup_order(node: &Node, keys: Vec<String>, now: Option<u64>) -> Vec<String> {
    let p = node.policy_at(now);
    let st = node.read();
    let views = Views::new(&*st, &p);
    let (mut first, mut rest) = (Vec::new(), Vec::new());
    for k in keys {
        let verified = st.by_product(&k).iter().filter_map(|id| st.get(id)).any(|r| {
            r.kind() == perch_core::kinds::names::PRODUCT
                && views.verified_author(&r.author())
                && views.chain_status(&r.id()) == Some(perch_views::Status::Visible)
        });
        if verified { first.push(k) } else { rest.push(k) }
    }
    first.extend(rest);
    first
}

async fn get_product_records(
    State(h): State<Http>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(key): Path<String>,
    RawQuery(raw): RawQuery,
) -> ApiResult {
    let now = q_u64(&query(&raw), "now")?;
    let _g = h.heavy(addr, &headers, 1.0).await?;
    let (lines, _) = h.compute(move |n| related_lines(n, &[key], now, None)).await?;
    Ok(jsonl_response(lines, Some(&h.node.policy_hash)))
}

async fn lookup(
    State(h): State<Http>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(prefix): Path<String>,
    RawQuery(raw): RawQuery,
) -> ApiResult {
    if prefix.len() < limits::LOOKUP_MIN
        || prefix.len() > limits::LOOKUP_MAX
        || !prefix.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(ApiError::bad(
            "bad_prefix",
            format!("prefix must be {} to {} lowercase hex characters", limits::LOOKUP_MIN, limits::LOOKUP_MAX),
        ));
    }
    let now = q_u64(&query(&raw), "now")?;
    let _g = h.heavy(addr, &headers, 2.0).await?;
    let budget = std::time::Duration::from_millis(h.node.config.lookup_budget_ms);
    let (lines, done, total) = h
        .compute(move |n| {
            let deadline = Instant::now() + budget;
            let keys: Vec<String> = n
                .read()
                .product_keys()
                .into_iter()
                .filter(|k| perch_core::ids::encode_hex(&perch_core::sha256(k.as_bytes())).starts_with(&prefix))
                .collect();
            let keys = lookup_order(n, keys, now);
            let total = keys.len();
            let (lines, done) = related_lines(n, &keys, now, Some(deadline));
            (lines, done, total)
        })
        .await?;
    let mut r = jsonl_response(lines, Some(&h.node.policy_hash));
    if done < total
        && let Ok(v) = HeaderValue::from_str(&format!("{done}/{total}"))
    {
        r.headers_mut().insert(TRUNCATED_HEADER, v);
    }
    Ok(r)
}

async fn get_listing(
    State(h): State<Http>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(id): Path<String>,
    RawQuery(raw): RawQuery,
) -> ApiResult {
    let id = parse_id(&id)?;
    let p = h.node.policy_at(q_u64(&query(&raw), "now")?);
    let _g = h.heavy(addr, &headers, 1.0).await?;
    let v = h.compute(move |n| Views::new(&*n.read(), &p).listing(&id).map(|v| canonical_json(&v))).await?;
    let v = v.ok_or_else(|| ApiError::not_found(format!("listing {id} is not known to this node")))?;
    Ok(view_text(&h.node, v))
}

async fn stats(State(h): State<Http>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap) -> ApiResult {
    let _g = h.heavy(addr, &headers, 1.0).await?;
    let s = h.compute(|n| canonical_json(&perch_views::stats(&*n.read()))).await?;
    Ok(view_text(&h.node, s))
}

/// Streams a snapshot of the dataset: the record handles are copied under
/// the lock (no record content is copied), then lines are encoded 500 at a
/// time as the client reads (audit #10).
async fn export(State(h): State<Http>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap) -> ApiResult {
    let _g = h.heavy(addr, &headers, limits::EXPORT_COST).await?;
    let recs: Vec<Record> = h.compute(|n| n.read().export_records().collect()).await?;
    let stream = futures_util::stream::unfold((recs, 0usize), |(recs, i)| async move {
        if i >= recs.len() {
            return None;
        }
        let end = (i + 500).min(recs.len());
        let mut s = String::new();
        for r in &recs[i..end] {
            s.push_str(&r.to_json_string());
            s.push('\n');
        }
        Some((Ok::<Bytes, Infallible>(Bytes::from(s)), (recs, end)))
    });
    let body = Body::from_stream(stream);
    Ok((StatusCode::OK, [(header::CONTENT_TYPE, "application/x-ndjson")], body).into_response())
}

async fn sync_ws(
    State(h): State<Http>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> ApiResult {
    // The per-address cap applies to the forwarded client when the proxy header is trusted.
    let addr = SocketAddr::new(h.client_ip(addr, &headers), addr.port());
    // Check caps and register the session in one step (audit #14).
    let (id, kill) = h.node.reserve_inbound(addr).map_err(|m| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "too_many_sessions", m))?;
    let node = h.node.clone();
    let node2 = h.node.clone();
    Ok(ws
        .max_message_size(crate::ws::MAX_WS_MESSAGE)
        .max_frame_size(crate::ws::MAX_WS_MESSAGE)
        .on_failed_upgrade(move |_| node2.unregister_session(id))
        .on_upgrade(move |socket| crate::ws::serve_axum(node, socket, addr, id, kill)))
}
