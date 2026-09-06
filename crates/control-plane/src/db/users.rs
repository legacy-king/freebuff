use sqlx::PgPool;
use uuid::Uuid;

use freebuff_shared::{AppError, CreateUser, User};

pub async fn create_user(pool: &PgPool, input: &CreateUser) -> Result<User, AppError> {
    let password_hash = hash_password(&input.password)?;

    let user = sqlx::query_as::<_, User>(
        r#"
        INSERT INTO users (email, name, password_hash)
        VALUES ($1, $2, $3)
        RETURNING *
        "#,
    )
    .bind(&input.email)
    .bind(&input.name)
    .bind(&password_hash)
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

pub async fn get_user_by_email(pool: &PgPool, email: &str) -> Result<User, AppError> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1")
        .bind(email)
        .fetch_one(pool)
        .await?;

    Ok(user)
}

pub async fn verify_password(pool: &PgPool, email: &str, password: &str) -> Result<User, AppError> {
    let user = get_user_by_email(pool, email).await?;

    let parsed_hash = argon2::password_hash::PasswordHash::new(&user.password_hash)
        .map_err(|e| AppError::Internal(format!("Invalid password hash: {}", e)))?;

    use argon2::password_hash::PasswordVerifier;
    argon2::Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .map_err(|_| AppError::Unauthorized("Invalid credentials".into()))?;

    Ok(user)
}

fn hash_password(password: &str) -> Result<String, AppError> {
    use argon2::password_hash::{rand_core::OsRng, PasswordHasher, SaltString};
    use argon2::Argon2;

    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(format!("Failed to hash password: {}", e)))?;

    Ok(hash.to_string())
}
