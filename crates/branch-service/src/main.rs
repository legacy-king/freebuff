mod branch_manager;
mod base_backup;
mod restore;

use axum::{routing::{get, post}, Router};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use freebuff_shared::Config;

#[derive(Clone)]
pub struct BranchState {
    pub db: sqlx::PgPool,
    pub config: Config,
    pub manager: Arc<branch_manager::BranchManager>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "freebuff_branch_service=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::from_env();

    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;

    let manager = Arc::new(branch_manager::BranchManager::new(config.clone()).await?);

    let state = BranchState {
        db,
        config: config.clone(),
        manager,
    };

    let app = Router::new()
        .route("/health", get(branch_manager::health_check))
        // Branch CRUD
        .route("/v1/branches", post(branch_manager::create_branch))
        .route("/v1/branches/:branch_id", get(branch_manager::get_branch))
        .route("/v1/branches/:branch_id/status", get(branch_manager::branch_status))
        // Base backup
        .route("/v1/branches/:branch_id/backup", post(base_backup::create_backup))
        .route("/v1/branches/:branch_id/backup/restore", post(base_backup::restore_backup))
        // PITR
        .route("/v1/branches/:branch_id/restore", post(restore::restore_point_in_time))
        // Middleware
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("{}:{}", config.api_host, 5002);
    tracing::info!("Branch service listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
