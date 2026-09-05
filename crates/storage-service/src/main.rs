use axum::{routing::{get, post, delete}, Router};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod routes;

#[derive(Clone)]
pub struct StorageState {
    pub config: freebuff_shared::Config,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "freebuff_storage=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = freebuff_shared::Config::from_env();

    let state = StorageState { config: config.clone() };

    let app = Router::new()
        .route("/health", get(routes::health::health_check))
        .route("/storage/v1/object/:bucket/*path", post(routes::storage::upload_object))
        .route("/storage/v1/object/:bucket/*path", get(routes::storage::get_object))
        .route("/storage/v1/object/:bucket/*path", delete(routes::storage::delete_object))
        .route("/storage/v1/bucket", post(routes::storage::create_bucket))
        .route("/storage/v1/bucket/:bucket", get(routes::storage::get_bucket))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("{}:{}", config.api_host, 5000);
    tracing::info!("Storage service listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
