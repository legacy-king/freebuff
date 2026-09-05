use sqlx::PgPool;
use uuid::Uuid;

use freebuff_shared::{AppError, Branch, BranchStatus, CreateBranch, UpdateBranch};

pub async fn list_branches(
    pool: &PgPool,
    project_id: Uuid,
    offset: u32,
    limit: u32,
) -> Result<Vec<Branch>, AppError> {
    let branches = sqlx::query_as::<_, Branch>(
        "SELECT * FROM branches WHERE project_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(project_id)
    .bind(limit as i32)
    .bind(offset as i32)
    .fetch_all(pool)
    .await?;

    Ok(branches)
}

pub async fn get_branch(pool: &PgPool, project_id: Uuid, branch_id: Uuid) -> Result<Branch, AppError> {
    let branch = sqlx::query_as::<_, Branch>(
        "SELECT * FROM branches WHERE project_id = $1 AND id = $2",
    )
    .bind(project_id)
    .bind(branch_id)
    .fetch_one(pool)
    .await?;

    Ok(branch)
}

pub async fn get_default_branch(pool: &PgPool, project_id: Uuid) -> Result<Branch, AppError> {
    let branch = sqlx::query_as::<_, Branch>(
        "SELECT * FROM branches WHERE project_id = $1 AND is_default = TRUE",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;

    Ok(branch)
}

pub async fn create_branch(
    pool: &PgPool,
    project_id: Uuid,
    input: &CreateBranch,
    slug: &str,
) -> Result<Branch, AppError> {
    let branch = sqlx::query_as::<_, Branch>(
        r#"
        INSERT INTO branches (project_id, name, slug, parent_branch_id, parent_lsn, status, is_default)
        VALUES ($1, $2, $3, $4, $5, $6, FALSE)
        RETURNING *
        "#,
    )
    .bind(project_id)
    .bind(&input.name)
    .bind(slug)
    .bind(input.parent_branch_id)
    .bind(input.parent_lsn)
    .bind(BranchStatus::Creating)
    .fetch_one(pool)
    .await?;

    Ok(branch)
}

pub async fn create_main_branch(pool: &PgPool, project_id: Uuid) -> Result<Branch, AppError> {
    let branch = sqlx::query_as::<_, Branch>(
        r#"
        INSERT INTO branches (project_id, name, slug, status, is_default)
        VALUES ($1, 'main', 'main', 'active', TRUE)
        RETURNING *
        "#,
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;

    Ok(branch)
}

pub async fn update_branch_status(
    pool: &PgPool,
    project_id: Uuid,
    branch_id: Uuid,
    status: BranchStatus,
) -> Result<Branch, AppError> {
    let branch = sqlx::query_as::<_, Branch>(
        "UPDATE branches SET status = $3 WHERE project_id = $1 AND id = $2 RETURNING *",
    )
    .bind(project_id)
    .bind(branch_id)
    .bind(status)
    .fetch_one(pool)
    .await?;

    Ok(branch)
}

pub async fn update_branch(
    pool: &PgPool,
    project_id: Uuid,
    branch_id: Uuid,
    input: &UpdateBranch,
) -> Result<Branch, AppError> {
    let branch = sqlx::query_as::<_, Branch>(
        "UPDATE branches SET name = COALESCE($3, name) WHERE project_id = $1 AND id = $2 RETURNING *",
    )
    .bind(project_id)
    .bind(branch_id)
    .bind(&input.name)
    .fetch_one(pool)
    .await?;

    Ok(branch)
}

pub async fn delete_branch(pool: &PgPool, project_id: Uuid, branch_id: Uuid) -> Result<(), AppError> {
    sqlx::query("DELETE FROM branches WHERE project_id = $1 AND id = $2 AND is_default = FALSE")
        .bind(project_id)
        .bind(branch_id)
        .execute(pool)
        .await?;

    Ok(())
}
