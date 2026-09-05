use axum::{extract::State, Json};
use axum_extra::headers::authorization::Bearer;
use axum_extra::TypedHeader;
use serde::{Deserialize, Serialize};

use crate::AuthState;
use freebuff_shared::{AppError, UserPublic};

#[derive(Debug, Deserialize)]
pub struct SignupRequest {
    pub email: String,
    pub password: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct TokenRequest {
    pub grant_type: String,
    pub email: Option<String>,
    pub password: Option<String>,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub refresh_token: String,
    pub user: UserPublic,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub email: Option<String>,
    pub password: Option<String>,
    pub data: Option<serde_json::Value>,
}

pub async fn signup(
    State(state): State<AuthState>,
    Json(input): Json<SignupRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    let user = crate::db::create_user(&state.db, &input.email, &input.password).await?;

    let token = freebuff_shared::auth::create_access_token(
        user.id,
        &user.email,
        &state.config.jwt_secret,
        state.config.jwt_expiration_secs,
        None,
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(TokenResponse {
        access_token: token,
        token_type: "bearer".into(),
        expires_in: state.config.jwt_expiration_secs,
        refresh_token: "refresh_placeholder".into(),
        user: UserPublic {
            id: user.id,
            email: user.email,
            name: user.name,
            created_at: user.created_at,
        },
    }))
}

pub async fn token(
    State(state): State<AuthState>,
    Json(input): Json<TokenRequest>,
) -> Result<Json<TokenResponse>, AppError> {
    match input.grant_type.as_str() {
        "password" => {
            let email = input.email.ok_or_else(|| AppError::BadRequest("email required".into()))?;
            let password = input.password.ok_or_else(|| AppError::BadRequest("password required".into()))?;

            let user = crate::db::verify_password(&state.db, &email, &password).await?;

            let token = freebuff_shared::auth::create_access_token(
                user.id,
                &user.email,
                &state.config.jwt_secret,
                state.config.jwt_expiration_secs,
                None,
            )
            .map_err(|e| AppError::Internal(e.to_string()))?;

            Ok(Json(TokenResponse {
                access_token: token,
                token_type: "bearer".into(),
                expires_in: state.config.jwt_expiration_secs,
                refresh_token: "refresh_placeholder".into(),
                user: UserPublic {
                    id: user.id,
                    email: user.email,
                    name: user.name,
                    created_at: user.created_at,
                },
            }))
        }
        "refresh_token" => {
            Err(AppError::BadRequest("Refresh token flow not yet implemented".into()))
        }
        _ => Err(AppError::BadRequest(format!("Unsupported grant type: {}", input.grant_type))),
    }
}

pub async fn get_user(
    State(state): State<AuthState>,
    TypedHeader(bearer): TypedHeader<Bearer>,
) -> Result<Json<UserPublic>, AppError> {
    let claims = freebuff_shared::auth::decode_access_token(
        bearer.token(),
        &state.config.jwt_secret,
    )
    .map_err(|_| AppError::Unauthorized("Invalid token".into()))?;

    let user = crate::db::get_user_by_id(&state.db, &claims.user_id()).await?;

    Ok(Json(UserPublic {
        id: user.id,
        email: user.email,
        name: user.name,
        created_at: user.created_at,
    }))
}

pub async fn update_user(
    State(state): State<AuthState>,
    TypedHeader(bearer): TypedHeader<Bearer>,
    Json(input): Json<UpdateUserRequest>,
) -> Result<Json<UserPublic>, AppError> {
    let claims = freebuff_shared::auth::decode_access_token(
        bearer.token(),
        &state.config.jwt_secret,
    )
    .map_err(|_| AppError::Unauthorized("Invalid token".into()))?;

    let user = crate::db::update_user(&state.db, &claims.user_id(), input.email.as_deref(), input.password.as_deref()).await?;

    Ok(Json(UserPublic {
        id: user.id,
        email: user.email,
        name: user.name,
        created_at: user.created_at,
    }))
}

pub async fn logout(
    State(_state): State<AuthState>,
    TypedHeader(_bearer): TypedHeader<Bearer>,
) -> Result<Json<serde_json::Value>, AppError> {
    // In production, this would invalidate the refresh token
    Ok(Json(serde_json::json!({ "message": "Logged out" })))
}
