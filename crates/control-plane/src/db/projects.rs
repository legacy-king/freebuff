use sqlx::PgPool;
use uuid::Uuid;

use freebuff_shared::{AppError, CreateProject, Project, ProjectStatus, UpdateProject};

pub async fn list_projects(pool: &PgPool, offset: u32, limit: u32) -> Result<Vec<Project>, AppError> {
    let projects = sqlx::query_as::<_, Project>(
        "SELECT * FROM projects ORDER BY created_at DESC LIMIT $1 OFFSET $2",
    )
    .bind(limit as i32)
    .bind(offset as i32)
    .fetch_all(pool)
    .await?;

    Ok(projects)
}

/// Active projects with a known database host, used by the storage metering
/// sampler to measure live database size.
pub async fn active_projects(pool: &PgPool) -> Result<Vec<Project>, AppError> {
    let projects = sqlx::query_as::<_, Project>(
        "SELECT * FROM projects WHERE status = 'active' AND database_host IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    Ok(projects)
}

pub async fn get_project(pool: &PgPool, project_id: Uuid) -> Result<Project, AppError> {
    let project = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_one(pool)
        .await?;

    Ok(project)
}

pub async fn create_project(
    pool: &PgPool,
    org_id: Uuid,
    input: &CreateProject,
    slug: &str,
) -> Result<Project, AppError> {
    let project = sqlx::query_as::<_, Project>(
        r#"
        INSERT INTO projects (org_id, name, slug, region, status)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING *
        "#,
    )
    .bind(org_id)
    .bind(&input.name)
    .bind(slug)
    .bind(input.region.as_deref().unwrap_or("us-east-1"))
    .bind(ProjectStatus::Creating)
    .fetch_one(pool)
    .await?;

    Ok(project)
}

pub async fn update_project_status(
    pool: &PgPool,
    project_id: Uuid,
    status: ProjectStatus,
) -> Result<Project, AppError> {
    let project = sqlx::query_as::<_, Project>(
        "UPDATE projects SET status = $2 WHERE id = $1 RETURNING *",
    )
    .bind(project_id)
    .bind(status)
    .fetch_one(pool)
    .await?;

    Ok(project)
}

pub async fn set_project_database(
    pool: &PgPool,
    project_id: Uuid,
    host: &str,
    port: i32,
    database_name: &str,
) -> Result<Project, AppError> {
    let project = sqlx::query_as::<_, Project>(
        r#"
        UPDATE projects
        SET database_host = $2, database_port = $3, database_name = $4, status = 'active'
        WHERE id = $1
        RETURNING *
        "#,
    )
    .bind(project_id)
    .bind(host)
    .bind(port)
    .bind(database_name)
    .fetch_one(pool)
    .await?;

    Ok(project)
}

pub async fn update_project(
    pool: &PgPool,
    project_id: Uuid,
    input: &UpdateProject,
) -> Result<Project, AppError> {
    let project = sqlx::query_as::<_, Project>(
        "UPDATE projects SET name = COALESCE($2, name) WHERE id = $1 RETURNING *",
    )
    .bind(project_id)
    .bind(&input.name)
    .fetch_one(pool)
    .await?;

    Ok(project)
}

pub async fn delete_project(pool: &PgPool, project_id: Uuid) -> Result<(), AppError> {
    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(project_id)
        .execute(pool)
        .await?;

    Ok(())
}
