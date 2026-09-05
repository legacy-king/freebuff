use axum::{
    extract::{Path, Query, State},
    Json,
};
use uuid::Uuid;

use crate::AppState;
use freebuff_shared::{api::ApiResponse, AppError, Branch, CreateBranch, PaginationParams, UpdateBranch};

pub async fn list_branches(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<Vec<Branch>>, AppError> {
    let branches = crate::db::branches::list_branches(&state.db, project_id, pagination.offset(), pagination.limit()).await?;
    Ok(Json(branches))
}

pub async fn create_branch(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<CreateBranch>,
) -> Result<Json<ApiResponse<Branch>>, AppError> {
    let branch = crate::services::branch_service::create_branch(&state, project_id, input).await?;
    Ok(Json(ApiResponse::new(branch)))
}

pub async fn get_branch(
    State(state): State<AppState>,
    Path((project_id, branch_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ApiResponse<Branch>>, AppError> {
    let branch = crate::db::branches::get_branch(&state.db, project_id, branch_id).await?;
    Ok(Json(ApiResponse::new(branch)))
}

pub async fn update_branch(
    State(state): State<AppState>,
    Path((project_id, branch_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<UpdateBranch>,
) -> Result<Json<ApiResponse<Branch>>, AppError> {
    let branch = crate::db::branches::update_branch(&state.db, project_id, branch_id, &input).await?;
    Ok(Json(ApiResponse::new(branch)))
}

pub async fn delete_branch(
    State(state): State<AppState>,
    Path((project_id, branch_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<ApiResponse<freebuff_shared::api::MessageResponse>>, AppError> {
    crate::db::branches::delete_branch(&state.db, project_id, branch_id).await?;
    Ok(Json(ApiResponse::new(freebuff_shared::api::MessageResponse {
        message: "Branch deleted".into(),
    })))
}
