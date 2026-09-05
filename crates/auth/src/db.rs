use sqlx::PgPool;
use uuid::Uuid;

use freebuff_shared::{AppError, User};

pub async fn create_user(pool: &PgPool, email: &str, password: &str) -> Result<User, AppError> {
    use argon2::password_hash::{rand_core::OsRng, PasswordHasher, SaltString};
    use argon2::Argon2;

    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(format!("Failed to hash password: {}", e)))?
        .to_string();

    let user = sqlx::query_as::<_, User>(
        r#"
        INSERT INTO users (email, password_hash)
        VALUES ($1, $2)
        RETURNING *
        "#,
    )
    .bind(email)
    .bind(hash)
    .fetch_one(pool)
    .await?;

    Ok(user)
}

pub async fn get_user_by_id(pool: &PgPool, user_id: Uuid) -> Result<User, AppError> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await?;

    Ok(user)
}

pub async fn verify_password(pool: &PgPool, email: &str, password: &str) -> Result<User, AppError> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1")
        .bind(email)
        .fetch_one(pool)
        .await
        .map_err(|_| AppError::Unauthorized("Invalid credentials".into()))?;

    let parsed_hash = argon2::password_hash::PasswordHash::new(&user.password_hash)
        .map_err(|e| AppError::Internal(format!("Invalid hash: {}", e)))?;

    argon2::password_hash::PasswordHash::verify_password(password.as_bytes(), &parsed_hash)
        .map_err(|_| AppError::Unauthorized("Invalid credentials".into()))?;

    Ok(user)
}

pub async fn update_user(
    pool: &PgPool,
    user_id: Uuid,
    email: Option<&str>,
    password: Option<&str>,
) -> Result<User, AppError> {
    if let Some(new_email) = email {
        let user = sqlx::query_as::<_, User>(
            "UPDATE users SET email = $2 WHERE id = $1 RETURNING *",
        )
        .bind(user_id)
        .bind(new_email)
        .fetch_one(pool)
        .await?;
        return Ok(user);
    }

    if let Some(new_password) = password {
        use argon2::password_hash::{rand_core::OsRng, PasswordHasher, SaltString};
        use argon2::Argon2;

        let salt = SaltString::generate(&mut OsRng);
        let hash = Argon2::default()
            .hash_password(new_password.as_bytes(), &salt)
            .map_err(|e| AppError::Internal(format!("Failed to hash password: {}", e)))?
            .to_string();

        let user = sqlx::query_as::<_, User>(
            "UPDATE users SET password_hash = $2 WHERE id = $1 RETURNING *",
        )
        .bind(user_id)
        .bind(hash)
        .fetch_one(pool)
        .await?;
        return Ok(user);
    }

    get_user_by_id(pool, user_id).await
}
