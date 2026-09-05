use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::GatewayState;
use freebuff_shared::AppError;

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Serialize)]
pub struct RestResponse {
    pub data: Value,
    pub count: Option<i64>,
}

pub async fn list_rows(
    State(_state): State<GatewayState>,
    Path(table): Path<String>,
    Query(query): Query<RestQuery>,
) -> Result<Json<RestResponse>, AppError> {
    tracing::info!("GET /rest/v1/{} with query: {:?}", table, query);

    // In production, this would:
    // 1. Validate the API key from the Authorization header
    // 2. Resolve the project's database connection
    // 3. Build and execute the SQL query
    // 4. Apply RLS policies
    // 5. Return the results

    Ok(Json(RestResponse {
        data: json!([]),
        count: Some(0),
    }))
}

pub async fn insert_rows(
    State(_state): State<GatewayState>,
    Path(table): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<RestResponse>, AppError> {
    tracing::info!("POST /rest/v1/{} with body: {:?}", table, body);

    Ok(Json(RestResponse {
        data: body,
        count: Some(1),
    }))
}

pub async fn update_rows(
    State(_state): State<GatewayState>,
    Path(table): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<RestResponse>, AppError> {
    tracing::info!("PATCH /rest/v1/{} with body: {:?}", table, body);

    Ok(Json(RestResponse {
        data: body,
        count: Some(1),
    }))
}

pub async fn delete_rows(
    State(_state): State<GatewayState>,
    Path(table): Path<String>,
) -> Result<Json<Value>, AppError> {
    tracing::info!("DELETE /rest/v1/{}", table);

    Ok(Json(json!({
        "data": null,
        "count": 0,
    })))
}
