use axum::{
    extract::{Path, Query, State},
    Json,
};
use uuid::Uuid;

use crate::AppState;
use freebuff_shared::{
    api::ApiResponse, AppError, ConnectionInfo, CreateProject, PaginationParams, Project,
    UpdateProject,
};

pub async fn list_projects(
    State(state): State<AppState>,
    Query(pagination): Query<PaginationParams>,
) -> Result<Json<Vec<Project>>, AppError> {
    let projects = crate::db::projects::list_projects(&state.db, pagination.offset(), pagination.limit()).await?;
    Ok(Json(projects))
}

pub async fn create_project(
    State(state): State<AppState>,
    Json(input): Json<CreateProject>,
) -> Result<Json<ApiResponse<Project>>, AppError> {
    let project = crate::services::project_service::create_project(&state, input).await?;
    Ok(Json(ApiResponse::new(project)))
}

pub async fn get_project(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<ApiResponse<Project>>, AppError> {
    let project = crate::db::projects::get_project(&state.db, project_id).await?;
    Ok(Json(ApiResponse::new(project)))
}

pub async fn update_project(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<UpdateProject>,
) -> Result<Json<ApiResponse<Project>>, AppError> {
    let project = crate::db::projects::update_project(&state.db, project_id, &input).await?;
    Ok(Json(ApiResponse::new(project)))
}

pub async fn delete_project(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<ApiResponse<freebuff_shared::api::MessageResponse>>, AppError> {
    crate::db::projects::delete_project(&state.db, project_id).await?;
    Ok(Json(ApiResponse::new(freebuff_shared::api::MessageResponse {
        message: "Project deleted".into(),
    })))
}

pub async fn get_connection_info(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<ApiResponse<ConnectionInfo>>, AppError> {
    let project = crate::db::projects::get_project(&state.db, project_id).await?;

    let conn_info = ConnectionInfo {
        host: project.database_host.unwrap_or_else(|| "localhost".into()),
        port: project.database_port.unwrap_or(5432),
        database: project.database_name.unwrap_or_else(|| "postgres".into()),
        role: "postgres".into(),
        password: "password_placeholder".into(),
        ssl_mode: "require".into(),
    };

    Ok(Json(ApiResponse::new(conn_info)))
}
