use axum::{extract::State, Json};
use axum_extra::headers::authorization::Bearer;
use axum_extra::TypedHeader;

use crate::AppState;
use freebuff_shared::{AppError, AuthResponse, CreateUser, LoginUser, UserPublic};

pub async fn register(
    State(state): State<AppState>,
    Json(input): Json<CreateUser>,
) -> Result<Json<AuthResponse>, AppError> {
    let user = crate::db::users::create_user(&state.db, &input).await?;

    let token = freebuff_shared::auth::create_access_token(
        user.id,
        &user.email,
        &state.config.jwt_secret,
        state.config.jwt_expiration_secs,
        None,
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(AuthResponse {
        access_token: token,
        refresh_token: "refresh_token_placeholder".into(),
        user: UserPublic {
            id: user.id,
            email: user.email,
            name: user.name,
            created_at: user.created_at,
        },
    }))
}

pub async fn login(
    State(state): State<AppState>,
    Json(input): Json<LoginUser>,
) -> Result<Json<AuthResponse>, AppError> {
    let user = crate::db::users::verify_password(&state.db, &input.email, &input.password).await?;

    let token = freebuff_shared::auth::create_access_token(
        user.id,
        &user.email,
        &state.config.jwt_secret,
        state.config.jwt_expiration_secs,
        None,
    )
    .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Json(AuthResponse {
        access_token: token,
        refresh_token: "refresh_token_placeholder".into(),
        user: UserPublic {
            id: user.id,
            email: user.email,
            name: user.name,
            created_at: user.created_at,
        },
    }))
}

pub async fn me(
    State(state): State<AppState>,
    TypedHeader(bearer): TypedHeader<Bearer>,
) -> Result<Json<UserPublic>, AppError> {
    let claims = freebuff_shared::auth::decode_access_token(
        bearer.token(),
        &state.config.jwt_secret,
    )
    .map_err(|_| AppError::Unauthorized("Invalid token".into()))?;

    let user = crate::db::users::get_user_by_id(&state.db, claims.user_id()).await?;

    Ok(Json(UserPublic {
        id: user.id,
        email: user.email,
        name: user.name,
        created_at: user.created_at,
    }))
}
