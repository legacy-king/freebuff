use axum::{extract::State, http::HeaderMap, Json};

use crate::AppState;
use freebuff_shared::{
    api::{ApiResponse, MessageResponse},
    AppError, InternalUsageIngest,
};

/// Internal usage ingestion endpoint.
///
/// Called by the API gateway (and any other service) to report usage events.
/// Authentication is via `Authorization: Bearer <INTERNAL_API_TOKEN>` or the
/// `X-Internal-Token` header.
pub async fn ingest_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<InternalUsageIngest>,
) -> Result<Json<ApiResponse<MessageResponse>>, AppError> {
    let header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .or_else(|| headers.get("x-internal-token").and_then(|v| v.to_str().ok()));
    let token = header.map(|h| h.strip_prefix("Bearer ").unwrap_or(h)).unwrap_or("");

    if token != state.config.internal_api_token {
        return Err(AppError::Unauthorized("Invalid internal token".into()));
    }

    let inserted = crate::services::usage_service::record_events(&state, &input.events).await?;
    tracing::info!("Ingested {} usage events from internal service", inserted);

    Ok(Json(ApiResponse::new(MessageResponse {
        message: format!("{} usage events recorded", inserted),
    })))
}