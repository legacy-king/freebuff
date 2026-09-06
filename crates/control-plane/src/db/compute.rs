use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use freebuff_shared::{AppError, ComputeEndpoint, ComputeSize, ComputeStatus};

pub async fn list_endpoints(
    pool: &PgPool,
    project_id: Uuid,
    offset: u32,
    limit: u32,
) -> Result<Vec<ComputeEndpoint>, AppError> {
    let endpoints = sqlx::query_as::<_, ComputeEndpoint>(
        "SELECT * FROM compute_endpoints WHERE project_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(project_id)
    .bind(limit as i32)
    .bind(offset as i32)
    .fetch_all(pool)
    .await?;

    Ok(endpoints)
}

pub async fn get_endpoint(pool: &PgPool, endpoint_id: Uuid) -> Result<ComputeEndpoint, AppError> {
    let endpoint = sqlx::query_as::<_, ComputeEndpoint>(
        "SELECT * FROM compute_endpoints WHERE id = $1",
    )
    .bind(endpoint_id)
    .fetch_one(pool)
    .await?;

    Ok(endpoint)
}

pub async fn create_endpoint(
    pool: &PgPool,
    project_id: Uuid,
    branch_id: Uuid,
) -> Result<ComputeEndpoint, AppError> {
    let compute_size = ComputeSize::Small;
    let max_connections = compute_size.max_connections();

    let endpoint = sqlx::query_as::<_, ComputeEndpoint>(
        r#"
        INSERT INTO compute_endpoints (branch_id, project_id, host, port, status, compute_size, max_connections)
        VALUES ($1, $2, 'localhost', 5432, 'running', $3, $4)
        RETURNING *
        "#,
    )
    .bind(branch_id)
    .bind(project_id)
    .bind(compute_size)
    .bind(max_connections)
    .fetch_one(pool)
    .await?;

    Ok(endpoint)
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RunningEndpoint {
    pub id: Uuid,
    pub project_id: Uuid,
    pub org_id: Uuid,
    pub compute_size: ComputeSize,
    pub last_accrued_at: Option<DateTime<Utc>>,
}

/// All currently running endpoints joined with their owning org,
/// used by the compute-hours metering task.
pub async fn running_endpoints(pool: &PgPool) -> Result<Vec<RunningEndpoint>, AppError> {
    let endpoints = sqlx::query_as::<_, RunningEndpoint>(
        r#"
        SELECT ce.id, ce.project_id, p.org_id, ce.compute_size, ce.last_accrued_at
        FROM compute_endpoints ce
        JOIN projects p ON p.id = ce.project_id
        WHERE ce.status = 'running'
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(endpoints)
}

pub async fn set_last_accrued_at(
    pool: &PgPool,
    endpoint_id: Uuid,
    timestamp: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query("UPDATE compute_endpoints SET last_accrued_at = $2 WHERE id = $1")
        .bind(endpoint_id)
        .bind(timestamp)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn set_endpoint_status(
    pool: &PgPool,
    project_id: Uuid,
    endpoint_id: Uuid,
    status: ComputeStatus,
) -> Result<ComputeEndpoint, AppError> {
    let endpoint = sqlx::query_as::<_, ComputeEndpoint>(
        "UPDATE compute_endpoints SET status = $3 WHERE project_id = $1 AND id = $2 RETURNING *",
    )
    .bind(project_id)
    .bind(endpoint_id)
    .bind(status)
    .fetch_one(pool)
    .await?;

    Ok(endpoint)
}
