//! Operator controls, served only on a loopback address (`--admin`).

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use perch_core::RecordId;
use serde_json::{Value, json};

use crate::http::ApiError;
use crate::node::Node;

pub fn router(node: Arc<Node>) -> Router {
    Router::new()
        .route("/admin/redact", post(redact))
        .route("/admin/removed", get(removed))
        .with_state(node)
}

async fn redact(State(node): State<Arc<Node>>, body: String) -> Result<Response, ApiError> {
    let v: Value = serde_json::from_str(&body).map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "bad_json", e.to_string()))?;
    let id = v
        .get("id")
        .and_then(|x| x.as_str())
        .and_then(|s| RecordId::from_hex(s).ok())
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "bad_id", "id must be a 64-character lowercase hex record ID"))?;
    let reason = v.get("reason").and_then(|x| x.as_str()).unwrap_or("");
    let note = v.get("note").and_then(|x| x.as_str()).map(str::to_owned);
    let n = node.clone();
    let reason = reason.to_string();
    let out = tokio::task::spawn_blocking(move || n.redact(id, &reason, note))
        .await
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", e.to_string()))??;
    Ok((StatusCode::OK, [(header::CONTENT_TYPE, "application/json")], out.to_string()).into_response())
}

async fn removed(State(node): State<Arc<Node>>) -> Response {
    let ids: Vec<String> = node.read().removed_ids().iter().map(|i| i.to_hex()).collect();
    (StatusCode::OK, [(header::CONTENT_TYPE, "application/json")], json!({ "removed": ids }).to_string()).into_response()
}
