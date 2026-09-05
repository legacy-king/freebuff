use axum::{
    extract::{Path, Query, State},
    Json,
};
use uuid::Uuid;

use crate::AppState;
use freebuff_shared::{api::ApiResponse, ApiKey, AppError, CreateApiKey, CreateApiKeyResponse, PaginationParams};

pub async fn list_keys(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<Vec<ApiKey>>, AppError> {
    let keys = crate::db::api_keys::list_keys(&state.db, project_id, pagination.offset(), pagination.limit()).await?;
    Ok(Json(keys))
}

pub async fn create_key(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<CreateApiKey>,
) -> Result<Json<ApiResponse<CreateApiKeyResponse>>, AppError> {
    let response = crate::services::api_key_service::create_api_key(&state, project_id, input).await?;
    Ok(Json(ApiResponse::new(response)))
}

pub async fn delete_key(
    State(state): State<AppState>,
    Path((project_id, key_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ApiResponse<freebuff_shared::api::MessageResponse>>, AppError> {
    crate::db::api_keys::delete_key(&state.db, project_id, key_id).await?;
    Ok(Json(ApiResponse::new(freebuff_shared::api::MessageResponse {
        message: "API key deleted".into(),
    })))
}
