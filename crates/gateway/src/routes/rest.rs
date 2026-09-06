use axum::{
    extract::{Path, Query, State},
    http::{Extensions, StatusCode},
    Json,
};
use serde_json::Value;

use crate::GatewayState;
use freebuff_shared::AppError;

/// Extension key used by the auth middleware to forward the validated Bearer
/// token into downstream handlers so the REST proxy can replay it to the
/// control plane.
#[derive(Debug, Clone)]
pub struct ForwardedToken(pub String);

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

/// Reverse-proxy the REST request to the control plane's real `/v1/projects`
/// (and future org-scoped tables). Currently only `projects` is wired; every
/// other table returns 501 so the unimplemented surface is explicit rather than
/// returning empty arrays.
async fn proxy_to_control_plane(
    state: &GatewayState,
    verb: reqwest::Method,
    table: &str,
    query: Option<RestQuery>,
    body: Option<Value>,
    extensions: &Extensions,
) -> Result<Json<RestResponse>, AppError> {
    if table != "projects" {
        return Err(AppError::BadRequest(format!(
            "REST table '{}' is not implemented yet — only 'projects' is wired",
            table
        )));
    }

    let token = extensions
        .get::<ForwardedToken>()
        .map(|t| t.0.clone());

    let client = reqwest::Client::new();
    let url = format!("{}/v1/projects", state.config.control_plane_url);

    let mut req = match (verb, body) {
        (reqwest::Method::GET, _) => client.get(&url),
        (reqwest::Method::POST, Some(b)) => client.post(&url).json(&b),
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

    // Forward pagination query params the control plane already supports.
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

    // The control plane returns either a bare array (GET /v1/projects) or an
    // envelope with a `data` field (POST /v1/projects -> ApiResponse<Project>).
    let rows: Vec<Value> = match &data {
        Value::Array(items) => items.clone(),
        Value::Object(map) => match map.get("data") {
            Some(Value::Array(items)) => items.clone(),
            Some(Value::Object(_)) => vec![map["data"].clone()],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    let count = match &data {
        Value::Array(items) => Some(items.len() as i64),
        _ => data
            .get("total")
            .and_then(|v| v.as_i64())
            .or(Some(rows.len() as i64)),
    };

    Ok(Json(RestResponse { data: rows, count }))
}

pub async fn list_rows(
    State(state): State<GatewayState>,
    Path(table): Path<String>,
    Query(query): Query<RestQuery>,
    extensions: Extensions,
) -> Result<Json<RestResponse>, AppError> {
    tracing::info!("GET /rest/v1/{} query", table);

    proxy_to_control_plane(
        &state,
        reqwest::Method::GET,
        &table,
        Some(query),
        None,
        &extensions,
    )
    .await
}

pub async fn insert_rows(
    State(state): State<GatewayState>,
    Path(table): Path<String>,
    extensions: Extensions,
    Json(body): Json<Value>,
) -> Result<Json<RestResponse>, AppError> {
    tracing::info!("POST /rest/v1/{} body=..", table);

    proxy_to_control_plane(
        &state,
        reqwest::Method::POST,
        &table,
        None,
        Some(body),
        &extensions,
    )
    .await
}

pub async fn update_rows(
    State(_state): State<GatewayState>,
    Path(_table): Path<String>,
    Json(_body): Json<Value>,
) -> Result<Json<RestResponse>, AppError> {
    Err(AppError::BadRequest("REST PATCH (update) is not implemented yet".into()))
}

pub async fn delete_rows(
    State(_state): State<GatewayState>,
    Path(_table): Path<String>,
) -> Result<Json<RestResponse>, AppError> {
    Err(AppError::BadRequest("REST DELETE is not implemented yet".into()))
}