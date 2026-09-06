use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};

use crate::GatewayState;
use freebuff_shared::AppError;

#[derive(Debug, serde::Deserialize)]
pub struct RestQuery {
    pub select: Option<String>,
    pub order: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
    pub eq: Option<String>,
    pub neq: Option<String>,
    pub gt: Option<String>,
    pub gte: Option<String>,
    pub lt: Option<String>,
    pub lte: Option<String>,
    pub like: Option<String>,
    pub ilike: Option<String>,
    pub in_: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct RestResponse {
    pub data: Vec<Value>,
    pub count: Option<i64>,
}

/// Resolve the requesting org from the Bearer JWT forwarded from the gateway's
/// auth middleware. The control plane validates the token; here we forward it on
/// every proxied request and let the control plane reject invalid/expired tokens.
fn extract_forwarded_token(req: &axum::http::Request<axum::body::Body>) -> Option<String> {
    req.headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|h| h.to_string())
}

/// Reverse-proxy the REST request to the control plane's real `/v1/projects`
/// (and future org-scoped tables). Currently only `projects` is wired; every other
/// table returns 501 so the unimplemented surface is explicit rather than returning
/// empty arrays.
async fn proxy_to_control_plane(
    state: &GatewayState,
    verb: reqwest::Method,
    table: &str,
    query: Option<RestQuery>,
    body: Option<Value>,
    request: &axum::http::Request<axum::body::Body>,
) -> Result<Json<RestResponse>, AppError> {
    if table != "projects" {
        return Err(AppError::BadRequest(format!(
            "REST table '{}' is not implemented yet — only 'projects' is wired",
            table
        )));
    }

    let token = extract_forwarded_token(request);
    let client = reqwest::Client::new();
    let url = format!("{}/v1/projects", state.config.control_plane_url);

    let mut req = match (verb, body) {
        ("GET", None) => client.get(&url),
        ("POST", Some(b)) => client.post(&url).json(&b),
        _ => {
            return Err(AppError::BadRequest(format!(
                "REST verb not supported for /rest/v1/{}",
                table
            )));
        }
    };

    if let Some(t) = token {
        req = req.header(axum::http::header::AUTHORIZATION, t);
    }

    // Forward pagination/filter query params the control plane already supports
    if let Some(q) = query {
        if let Some(limit) = q.limit {
            req = req.query(&[("per_page", limit.to_string())]);
        }
        if let Some(offset) = q.offset {
            req = req.query(&[("page", ((offset / 20) + 1).to_string())]);
        }
    }

    let resp = req.send().await.map_err(|e| {
        AppError::Internal(format!("control plane unreachable: {}", e))
    })?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return if status == StatusCode::UNAUTHORIZED {
            Err(AppError::Unauthorized("invalid or missing token".into()))
        } else {
            Err(AppError::Internal(text))
        };
    }

    let data: Value = serde_json::from_str(&text).map_err(|e| {
        AppError::Internal(format!("control plane returned non-JSON: {}", e))
    })?;

    let count = data.get("total").and_then(|v| v.as_i64());
    let rows: Vec<Value> = data
        .get("data")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    Ok(Json(RestResponse { data: rows, count }))
}

fn method_not_allowed() -> AppError {
    AppError::BadRequest("REST verb not supported".into())
}

fn not_implemented(msg: &str) -> AppError {
    AppError::BadRequest(msg.into())
}

fn bad_gateway(msg: impl Into<String>) -> AppError {
    AppError::Internal(msg.into())
}

pub async fn list_rows(
    State(state): State<GatewayState>,
    Path(table): Path<String>,
    Query(query): Query<RestQuery>,
    request: axum::extract::Request,
) -> Result<Json<RestResponse>, AppError> {
    tracing::info!("GET /rest/v1/{} org=?, query", table);

    proxy_to_control_plane(
        &state,
        reqwest::Method::GET,
        &table,
        Some(query),
        None,
        &request,
    )
    .await
}

pub async fn insert_rows(
    State(state): State<GatewayState>,
    Path(table): Path<String>,
    Json(body): Json<Value>,
    request: axum::extract::Request,
) -> Result<Json<RestResponse>, AppError> {
    tracing::info!("POST /rest/v1/{} org=? body=..", table);

    proxy_to_control_plane(
        &state,
        reqwest::Method::POST,
        &table,
        None,
        Some(body),
        &request,
    )
    .await
}

pub async fn update_rows(
    State(_state): State<GatewayState>,
    Path(_table): Path<String>,
    Json(_body): Json<Value>,
) -> Result<Json<RestResponse>, AppError> {
    Err(not_implemented("REST PATCH (update) is not implemented yet"))
}

pub async fn delete_rows(
    State(_state): State<GatewayState>,
    Path(_table): Path<String>,
) -> Result<Json<RestResponse>, AppError> {
    Err(not_implemented("REST DELETE is not implemented yet"))
}
