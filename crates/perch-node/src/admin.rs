//! Operator controls, served only on a loopback address (`--admin`).
//!
//! Every request must:
//! - carry `Authorization: Bearer <token>`, where the token is the content of
//!   `<data>/admin-token` (random, regenerated at each start, mode 0600);
//! - carry no `Origin` header, so no web page can drive the endpoint;
//! - name the admin address in `Host` (`127.0.0.1:<port>`, `localhost:<port>`,
//!   or `[::1]:<port>`), which defeats DNS rebinding;
//! - for `POST`, use `Content-Type: application/json`, which a page cannot
//!   send cross-origin without a preflight.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use perch_core::RecordId;
use serde_json::{Value, json};

use crate::http::ApiError;
use crate::node::Node;

/// State of the admin listener.
#[derive(Clone)]
pub struct Admin {
    pub node: Arc<Node>,
    pub token: Arc<String>,
    pub addr: SocketAddr,
}

/// Generates a random token and writes it to `<data>/admin-token` with mode 0600.
pub fn new_token(data: &std::path::Path) -> std::io::Result<(String, std::path::PathBuf)> {
    use std::io::Write;
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|e| std::io::Error::other(e.to_string()))?;
    let token = perch_core::ids::encode_hex(&b);
    let path = data.join("admin-token");
    let _ = std::fs::remove_file(&path);
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(&path)?;
    f.write_all(token.as_bytes())?;
    f.write_all(b"\n")?;
    f.sync_all()?;
    Ok((token, path))
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn deny(status: StatusCode, code: &'static str, msg: &str) -> Response {
    ApiError::new(status, code, msg).into_response()
}

async fn guard(State(a): State<Admin>, req: Request, next: Next) -> Response {
    let h = req.headers();
    if h.contains_key(header::ORIGIN) {
        return deny(StatusCode::FORBIDDEN, "cross_origin", "requests with an Origin header are refused");
    }
    let port = a.addr.port();
    let allowed = [format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}"), a.addr.to_string()];
    let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    if !allowed.iter().any(|x| x.eq_ignore_ascii_case(host)) {
        return deny(StatusCode::FORBIDDEN, "bad_host", "Host must be the admin address");
    }
    let auth = h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).unwrap_or("");
    let given = auth.strip_prefix("Bearer ").unwrap_or("").trim();
    if !ct_eq(given.as_bytes(), a.token.as_bytes()) {
        return deny(StatusCode::UNAUTHORIZED, "unauthorized", "missing or wrong bearer token (see <data>/admin-token)");
    }
    if req.method() == Method::POST {
        let ct = h.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
        if !ct.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json") {
            return deny(StatusCode::UNSUPPORTED_MEDIA_TYPE, "bad_content_type", "Content-Type must be application/json");
        }
    }
    next.run(req).await
}

pub fn router(state: Admin) -> Router {
    Router::new()
        .route("/admin/redact", post(redact))
        .route("/admin/removed", get(removed))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

async fn redact(State(a): State<Admin>, body: String) -> Result<Response, ApiError> {
    let v: Value = serde_json::from_str(&body).map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "bad_json", e.to_string()))?;
    let id = v
        .get("id")
        .and_then(|x| x.as_str())
        .and_then(|s| RecordId::from_hex(s).ok())
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "bad_id", "id must be a 64-character lowercase hex record ID"))?;
    let reason = v.get("reason").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let note = v.get("note").and_then(|x| x.as_str()).map(str::to_owned);
    let n = a.node.clone();
    let out = tokio::task::spawn_blocking(move || n.redact(id, &reason, note))
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string()))??;
    Ok((StatusCode::OK, [(header::CONTENT_TYPE, "application/json")], out.to_string()).into_response())
}

async fn removed(State(a): State<Admin>) -> Response {
    let ids: Vec<String> = a.node.read().removed_ids().iter().map(|i| i.to_hex()).collect();
    (StatusCode::OK, [(header::CONTENT_TYPE, "application/json")], json!({ "removed": ids }).to_string()).into_response()
}
