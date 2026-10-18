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

/// Shared state of the HTTP layer.
#[derive(Clone)]
pub struct Http {
    pub node: Arc<Node>,
    post_limit: Arc<Limiter>,
    heavy_limit: Arc<Limiter>,
    views: Arc<tokio::sync::Semaphore>,
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
        .expose_headers([header::HeaderName::from_static(POLICY_HASH_HEADER)]);
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
    fn client_ip(&self, addr: SocketAddr, headers: &HeaderMap) -> IpAddr {
        if addr.ip().is_loopback()
            && let Some(h) = &self.node.config.trust_proxy_header
            && let Some(v) = headers.get(h.as_str()).and_then(|v| v.to_str().ok())
            && let Some(ip) = v.split(',').next().and_then(|x| x.trim().parse::<IpAddr>().ok())
        {
            return ip;
        }
        addr.ip()
    }

    fn heavy(&self, addr: SocketAddr, headers: &HeaderMap, cost: f64) -> Result<(), ApiError> {
        if self.heavy_limit.take(self.client_ip(addr, headers), cost) {
            Ok(())
        } else {
            Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", "too many view requests from this address; retry later"))
        }
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
    if !h.post_limit.take(h.client_ip(addr, &headers), 1.0) {
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
    h.heavy(addr, &headers, 1.0)?;
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
    h.heavy(addr, &headers, 1.0)?;
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
    h.heavy(addr, &headers, 1.0)?;
    let k = key.clone();
    let v = h.compute(move |n| perch_views::product_view(&*n.read(), &p, &k).map(|v| canonical_json(&v))).await?;
    let v = v.ok_or_else(|| ApiError::not_found(format!("product {key:?} is not known to this node")))?;
    Ok(view_text(&h.node, v))
}

fn related_lines(node: &Node, keys: &[String], now: Option<u64>) -> Vec<String> {
    let p = node.policy_at(now);
    let st = node.read();
    let mut ids = std::collections::BTreeSet::new();
    for k in keys {
        ids.extend(perch_views::related_records(&*st, k, &p));
    }
    ids.into_iter().filter_map(|id| st.get(&id)).map(|r| r.to_json_string()).collect()
}

async fn get_product_records(
    State(h): State<Http>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(key): Path<String>,
    RawQuery(raw): RawQuery,
) -> ApiResult {
    let now = q_u64(&query(&raw), "now")?;
    h.heavy(addr, &headers, 1.0)?;
    let lines = h.compute(move |n| related_lines(n, &[key], now)).await?;
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
    h.heavy(addr, &headers, 2.0)?;
    let lines = h
        .compute(move |n| {
            let keys: Vec<String> = n
                .read()
                .product_keys()
                .into_iter()
                .filter(|k| perch_core::ids::encode_hex(&perch_core::sha256(k.as_bytes())).starts_with(&prefix))
                .collect();
            related_lines(n, &keys, now)
        })
        .await?;
    Ok(jsonl_response(lines, Some(&h.node.policy_hash)))
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
    h.heavy(addr, &headers, 1.0)?;
    let v = h.compute(move |n| Views::new(&*n.read(), &p).listing(&id).map(|v| canonical_json(&v))).await?;
    let v = v.ok_or_else(|| ApiError::not_found(format!("listing {id} is not known to this node")))?;
    Ok(view_text(&h.node, v))
}

async fn stats(State(h): State<Http>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap) -> ApiResult {
    h.heavy(addr, &headers, 1.0)?;
    let s = h.compute(|n| canonical_json(&perch_views::stats(&*n.read()))).await?;
    Ok(view_text(&h.node, s))
}

/// Streams a snapshot of the dataset: the record handles are copied under
/// the lock (no record content is copied), then lines are encoded 500 at a
/// time as the client reads (audit #10).
async fn export(State(h): State<Http>, ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap) -> ApiResult {
    h.heavy(addr, &headers, limits::EXPORT_COST)?;
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

async fn sync_ws(State(h): State<Http>, ConnectInfo(addr): ConnectInfo<SocketAddr>, ws: WebSocketUpgrade) -> ApiResult {
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
