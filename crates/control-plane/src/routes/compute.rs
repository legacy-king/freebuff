use axum::{
    extract::{Path, Query, State},
    Json,
};
use uuid::Uuid;

use crate::AppState;
use freebuff_shared::{api::ApiResponse, AppError, ComputeEndpoint, PaginationParams};

pub async fn list_endpoints(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<Vec<ComputeEndpoint>>, AppError> {
    let endpoints = crate::db::compute::list_endpoints(&state.db, project_id, pagination.offset(), pagination.limit()).await?;
    Ok(Json(endpoints))
}

pub async fn create_endpoint(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<freebuff_shared::api::IdResponse>,
) -> Result<Json<ApiResponse<ComputeEndpoint>>, AppError> {
    let branch_id = Uuid::parse_str(&input.id)
        .map_err(|_| AppError::BadRequest("Invalid branch ID".into()))?;

    let endpoint = crate::db::compute::create_endpoint(&state.db, project_id, branch_id).await?;
    Ok(Json(ApiResponse::new(endpoint)))
}

pub async fn stop_endpoint(
    State(state): State<AppState>,
    Path((project_id, endpoint_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ApiResponse<ComputeEndpoint>>, AppError> {
    let endpoint = crate::db::compute::set_endpoint_status(
        &state.db,
        project_id,
        endpoint_id,
        freebuff_shared::ComputeStatus::Stopped,
    ).await?;
    Ok(Json(ApiResponse::new(endpoint)))
}

pub async fn start_endpoint(
    State(state): State<AppState>,
    Path((project_id, endpoint_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ApiResponse<ComputeEndpoint>>, AppError> {
    let endpoint = crate::db::compute::set_endpoint_status(
        &state.db,
        project_id,
        endpoint_id,
        freebuff_shared::ComputeStatus::Running,
    ).await?;
    Ok(Json(ApiResponse::new(endpoint)))
}
