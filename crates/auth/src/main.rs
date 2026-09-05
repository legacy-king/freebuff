use axum::{routing::{get, post}, Router};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod routes;
mod db;

#[derive(Clone)]
pub struct AuthState {
    pub db: sqlx::PgPool,
    pub config: freebuff_shared::Config,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "freebuff_auth=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = freebuff_shared::Config::from_env();

    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;

    let state = AuthState { db, config: config.clone() };

    let app = Router::new()
        .route("/health", get(routes::health::health_check))
        .route("/auth/v1/signup", post(routes::auth::signup))
        .route("/auth/v1/token", post(routes::auth::token))
        .route("/auth/v1/user", get(routes::auth::get_user))
        .route("/auth/v1/user", put(routes::auth::update_user))
        .route("/auth/v1/logout", post(routes::auth::logout))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("{}:{}", config.api_host, 9999);
    tracing::info!("Auth service listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
