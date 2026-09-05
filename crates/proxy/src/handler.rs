use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};

use crate::ProxyState;
use freebuff_shared::{AppError, ComponentHealth, HealthStatus};

#[derive(Debug, Deserialize)]
pub struct ValidateRequest {
    pub api_key: String,
    pub project_id: String,
}

#[derive(Debug, Serialize)]
pub struct ValidateResponse {
    pub valid: bool,
    pub project_id: String,
    pub database_host: String,
    pub database_port: i32,
    pub database_name: String,
}

#[derive(Debug, Deserialize)]
pub struct ConnectRequest {
    pub api_key: String,
    pub project_id: String,
    pub database: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ConnectResponse {
    pub host: String,
    pub port: i32,
    pub database: String,
    pub role: String,
    pub ssl_required: bool,
}

pub async fn health_check() -> Json<HealthStatus> {
    Json(HealthStatus {
        status: "healthy".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        uptime_secs: 0,
        components: vec![ComponentHealth {
            name: "proxy".into(),
            status: "healthy".into(),
            message: None,
        }],
    })
}

pub async fn validate_connection(
    State(state): State<ProxyState>,
    Json(input): Json<ValidateRequest>,
) -> Result<Json<ValidateResponse>, AppError> {
    // In production, this would call the control plane to validate the API key
    // and get the database connection info
    tracing::info!(
        "Validating connection for project {}",
        input.project_id
    );

    // Placeholder response — real implementation validates via control plane
    Ok(Json(ValidateResponse {
        valid: true,
        project_id: input.project_id.clone(),
        database_host: format!("db-{}", input.project_id),
        database_port: 5432,
        database_name: format!("freebuff_{}", input.project_id),
    }))
}

pub async fn handle_connection(
    State(state): State<ProxyState>,
    Json(input): Json<ConnectRequest>,
) -> Result<Json<ConnectResponse>, AppError> {
    tracing::info!(
        "Connection request for project {}",
        input.project_id
    );

    // Get or create a connection pool for this project
    let pool = state.pool_manager.get_or_create_pool(&input.project_id).await?;

    // Get connection info from pool
    Ok(Json(ConnectResponse {
        host: pool.host.clone(),
        port: pool.port,
        database: pool.database.clone(),
        role: "freebuff_authenticator".into(),
        ssl_required: true,
    }))
}
