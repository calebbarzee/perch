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
use axum::http::{HeaderValue, Method, StatusCode, header};
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

/// Shared state of the HTTP layer.
#[derive(Clone)]
pub struct Http {
    pub node: Arc<Node>,
    post_buckets: Arc<Mutex<HashMap<IpAddr, (f64, Instant)>>>,
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

fn view_json<T: serde::Serialize>(node: &Node, v: &T) -> Response {
    with_policy_hash(json_response(StatusCode::OK, canonical_json(v)), &node.policy_hash)
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
    let state = Http { node: node.clone(), post_buckets: Arc::new(Mutex::new(HashMap::new())) };
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
            "authors": st.authors().len(),
            "forked_authors": st.forked_authors().len(),
            "products": st.product_keys().len(),
            "held": st.held_count(),
            "bodyless": st.bodyless_count(),
            "removed": st.removed_ids().len(),
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
    /// Token bucket per client IP for `POST /v1/records`. Loopback is exempt.
    fn allow_post(&self, ip: IpAddr) -> bool {
        if ip.is_loopback() {
            return true;
        }
        let mut m = self.post_buckets.lock().unwrap_or_else(|e| e.into_inner());
        if m.len() > 100_000 {
            m.clear();
        }
        let now = Instant::now();
        let e = m.entry(ip).or_insert((limits::POST_BURST, now));
        e.0 = (e.0 + now.duration_since(e.1).as_secs_f64() * limits::POST_RATE).min(limits::POST_BURST);
        e.1 = now;
        if e.0 >= 1.0 {
            e.0 -= 1.0;
            true
        } else {
            false
        }
    }
}

async fn post_records(State(h): State<Http>, ConnectInfo(addr): ConnectInfo<SocketAddr>, body: Body) -> ApiResult {
    if !h.allow_post(addr.ip()) {
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
        let v = perch_core::json::parse(trimmed, perch_core::limits::MAX_DEPTH + 1)
            .map_err(|e| ApiError::bad("bad_json", format!("JSON array: {e}")))?;
        let Value::Array(arr) = v else { return Err(ApiError::bad("bad_json", "expected a JSON array of records")) };
        if arr.len() > limits::POST_RECORDS {
            return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too_many_records", format!("{} records; limit {}", arr.len(), limits::POST_RECORDS)));
        }
        for (i, v) in arr.into_iter().enumerate() {
            items.push((i, Record::from_value(v).map_err(|e| e.to_string())));
        }
    } else {
        for (line, r) in perch_core::parse_jsonl(text) {
            if items.len() >= limits::POST_RECORDS {
                return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too_many_records", format!("more than {} records", limits::POST_RECORDS)));
            }
            items.push((line, r.map_err(|e| e.to_string())));
        }
    }
    let valid: Vec<Record> = items.iter().filter_map(|(_, r)| r.as_ref().ok().cloned()).collect();
    let outcomes = h.node.submit(valid)?;
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

async fn get_author(State(h): State<Http>, Path(id): Path<String>, RawQuery(raw): RawQuery) -> ApiResult {
    let a = parse_author(&id)?;
    let p = h.node.policy_at(q_u64(&query(&raw), "now")?);
    let v = {
        let st = h.node.read();
        perch_views::author_view(&*st, &p, &a)
    };
    let v = v.ok_or_else(|| ApiError::not_found(format!("author {a} is not known to this node")))?;
    Ok(view_json(&h.node, &v))
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

async fn search(State(h): State<Http>, RawQuery(raw): RawQuery) -> ApiResult {
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
    let hits = {
        let st = h.node.read();
        perch_views::search(&*st, &p, &text, &opts)
    };
    Ok(view_json(&h.node, &hits))
}

async fn get_product(State(h): State<Http>, Path(key): Path<String>, RawQuery(raw): RawQuery) -> ApiResult {
    let p = h.node.policy_at(q_u64(&query(&raw), "now")?);
    let v = {
        let st = h.node.read();
        perch_views::product_view(&*st, &p, &key)
    };
    let v = v.ok_or_else(|| ApiError::not_found(format!("product {key:?} is not known to this node")))?;
    Ok(view_json(&h.node, &v))
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

async fn get_product_records(State(h): State<Http>, Path(key): Path<String>, RawQuery(raw): RawQuery) -> ApiResult {
    let now = q_u64(&query(&raw), "now")?;
    let lines = related_lines(&h.node, &[key], now);
    Ok(jsonl_response(lines, Some(&h.node.policy_hash)))
}

async fn lookup(State(h): State<Http>, Path(prefix): Path<String>, RawQuery(raw): RawQuery) -> ApiResult {
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
    let keys: Vec<String> = {
        let st = h.node.read();
        st.product_keys()
            .into_iter()
            .filter(|k| perch_core::ids::encode_hex(&perch_core::sha256(k.as_bytes())).starts_with(&prefix))
            .collect()
    };
    let lines = related_lines(&h.node, &keys, now);
    Ok(jsonl_response(lines, Some(&h.node.policy_hash)))
}

async fn get_listing(State(h): State<Http>, Path(id): Path<String>, RawQuery(raw): RawQuery) -> ApiResult {
    let id = parse_id(&id)?;
    let p = h.node.policy_at(q_u64(&query(&raw), "now")?);
    let v = {
        let st = h.node.read();
        Views::new(&*st, &p).listing(&id)
    };
    let v = v.ok_or_else(|| ApiError::not_found(format!("listing {id} is not known to this node")))?;
    Ok(view_json(&h.node, &v))
}

async fn stats(State(h): State<Http>) -> Response {
    let s = {
        let st = h.node.read();
        perch_views::stats(&*st)
    };
    view_json(&h.node, &s)
}

async fn export(State(h): State<Http>) -> Response {
    let recs: Vec<Record> = h.node.read().export_records().collect();
    let chunks = recs.chunks(500).map(|c| {
        let mut s = String::new();
        for r in c {
            s.push_str(&r.to_json_string());
            s.push('\n');
        }
        Ok::<Bytes, Infallible>(Bytes::from(s))
    });
    let chunks: Vec<Result<Bytes, Infallible>> = chunks.collect();
    let body = Body::from_stream(futures_util::stream::iter(chunks));
    (StatusCode::OK, [(header::CONTENT_TYPE, "application/x-ndjson")], body).into_response()
}

async fn sync_ws(State(h): State<Http>, ConnectInfo(addr): ConnectInfo<SocketAddr>, ws: WebSocketUpgrade) -> ApiResult {
    {
        let peers = h.node.peers.lock().unwrap_or_else(|e| e.into_inner());
        let inbound: Vec<_> = peers.values().filter(|p| !p.outbound).collect();
        if inbound.len() >= limits::INBOUND_SESSIONS {
            return Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "too_many_sessions", "inbound session limit reached"));
        }
        if !addr.ip().is_loopback() && inbound.iter().filter(|p| p.remote.map(|r| r.ip()) == Some(addr.ip())).count() >= limits::INBOUND_PER_IP {
            return Err(ApiError::new(StatusCode::TOO_MANY_REQUESTS, "too_many_sessions", "at most 8 sessions per address"));
        }
    }
    let node = h.node.clone();
    Ok(ws
        .max_message_size(crate::ws::MAX_WS_MESSAGE)
        .max_frame_size(crate::ws::MAX_WS_MESSAGE)
        .on_upgrade(move |socket| crate::ws::serve_axum(node, socket, addr)))
}
