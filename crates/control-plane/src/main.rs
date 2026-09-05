mod routes;
mod db;
mod services;

use axum::{routing::{get, post, put, delete}, Router};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use freebuff_shared::Config;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub redis: redis::aio::ConnectionManager,
    pub config: Config,
    pub http: reqwest::Client,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "freebuff_control_plane=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::from_env();

    // Database connection pool
    let db = PgPoolOptions::new()
        .max_connections(20)
        .connect(&config.database_url)
        .await?;

    tracing::info!("Connected to database");

    // Redis connection
    let redis_client = redis::Client::open(config.redis_url.clone())?;
    let redis = redis::aio::ConnectionManager::new(redis_client).await?;

    tracing::info!("Connected to Redis");

    let state = AppState {
        db,
        redis,
        config: config.clone(),
        http: reqwest::Client::new(),
    };

    // Background metering tasks
    tokio::spawn(crate::services::usage_service::compute_accrual_loop(state.clone()));
    tokio::spawn(crate::services::usage_service::usage_report_loop(state.clone()));
    if config.storage_sample_secs > 0 {
        tokio::spawn(crate::services::usage_service::storage_sampler_loop(state.clone()));
    }

    let app = Router::new()
        // Health
        .route("/health", get(routes::health::health_check))
        // Auth
        .route("/v1/auth/register", post(routes::auth::register))
        .route("/v1/auth/login", post(routes::auth::login))
        .route("/v1/auth/me", get(routes::auth::me))
        // Projects
        .route("/v1/projects", get(routes::projects::list_projects))
        .route("/v1/projects", post(routes::projects::create_project))
        .route("/v1/projects/:project_id", get(routes::projects::get_project))
        .route("/v1/projects/:project_id", put(routes::projects::update_project))
        .route("/v1/projects/:project_id", delete(routes::projects::delete_project))
        .route("/v1/projects/:project_id/connection", get(routes::projects::get_connection_info))
        // Branches
        .route("/v1/projects/:project_id/branches", get(routes::branches::list_branches))
        .route("/v1/projects/:project_id/branches", post(routes::branches::create_branch))
        .route("/v1/projects/:project_id/branches/:branch_id", get(routes::branches::get_branch))
        .route("/v1/projects/:project_id/branches/:branch_id", put(routes::branches::update_branch))
        .route("/v1/projects/:project_id/branches/:branch_id", delete(routes::branches::delete_branch))
        // Compute Endpoints
        .route("/v1/projects/:project_id/compute", get(routes::compute::list_endpoints))
        .route("/v1/projects/:project_id/compute", post(routes::compute::create_endpoint))
        .route("/v1/projects/:project_id/compute/:endpoint_id/stop", post(routes::compute::stop_endpoint))
        .route("/v1/projects/:project_id/compute/:endpoint_id/start", post(routes::compute::start_endpoint))
        // API Keys
        .route("/v1/projects/:project_id/api-keys", get(routes::api_keys::list_keys))
        .route("/v1/projects/:project_id/api-keys", post(routes::api_keys::create_key))
        .route("/v1/projects/:project_id/api-keys/:key_id", delete(routes::api_keys::delete_key))
        // Billing & Usage Metering
        .route("/v1/billing/account", get(routes::billing::get_account))
        .route("/v1/billing/checkout", post(routes::billing::create_checkout_session))
        .route("/v1/billing/portal", post(routes::billing::create_portal_session))
        .route("/v1/billing/cancel", post(routes::billing::cancel_subscription))
        .route("/v1/billing/usage", get(routes::billing::get_usage))
        .route("/v1/billing/webhook", post(routes::billing::webhook))
        // Internal service-to-service
        .route("/v1/internal/usage", post(routes::internal::ingest_usage))
        // Middleware
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("{}:{}", config.api_host, config.api_port);
    tracing::info!("Control plane listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
