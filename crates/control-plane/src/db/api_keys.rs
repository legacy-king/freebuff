use sqlx::PgPool;
use uuid::Uuid;

use freebuff_shared::{ApiKey, ApiKeyType, AppError};

pub async fn list_keys(
    pool: &PgPool,
    project_id: Uuid,
    offset: u32,
    limit: u32,
) -> Result<Vec<ApiKey>, AppError> {
    let keys = sqlx::query_as::<_, ApiKey>(
        "SELECT * FROM api_keys WHERE project_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(project_id)
    .bind(limit as i32)
    .bind(offset as i32)
    .fetch_all(pool)
    .await?;

    Ok(keys)
}

pub async fn create_key(
    pool: &PgPool,
    project_id: Uuid,
    name: &str,
    key_hash: &str,
    key_prefix: &str,
    key_type: ApiKeyType,
    scopes: &[String],
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<ApiKey, AppError> {
    let key = sqlx::query_as::<_, ApiKey>(
        r#"
        INSERT INTO api_keys (project_id, name, key_hash, key_prefix, key_type, scopes, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING *
        "#,
    )
    .bind(project_id)
    .bind(name)
    .bind(key_hash)
    .bind(key_prefix)
    .bind(key_type)
    .bind(scopes)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;

    Ok(key)
}

pub async fn get_key_by_prefix(
    pool: &PgPool,
    project_id: Uuid,
    key_prefix: &str,
) -> Result<ApiKey, AppError> {
    let key = sqlx::query_as::<_, ApiKey>(
        "SELECT * FROM api_keys WHERE project_id = $1 AND key_prefix = $2",
    )
    .bind(project_id)
    .bind(key_prefix)
    .fetch_one(pool)
    .await?;

    Ok(key)
}

pub async fn delete_key(pool: &PgPool, project_id: Uuid, key_id: Uuid) -> Result<(), AppError> {
    sqlx::query("DELETE FROM api_keys WHERE project_id = $1 AND id = $2")
        .bind(project_id)
        .bind(key_id)
        .execute(pool)
        .await?;

    Ok(())
}
