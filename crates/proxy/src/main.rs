mod handler;
mod pool;
mod config;

use axum::{routing::{get, post}, Router};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
pub struct ProxyState {
    pub pool_manager: Arc<pool::PoolManager>,
    pub config: freebuff_shared::ProxyConfig,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "freebuff_proxy=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let proxy_config = freebuff_shared::ProxyConfig::from_env();

    let pool_manager = Arc::new(pool::PoolManager::new(proxy_config.clone()).await?);

    let state = ProxyState {
        pool_manager,
        config: proxy_config.clone(),
    };

    let app = Router::new()
        .route("/health", get(handler::health_check))
        .route("/proxy/validate", post(handler::validate_connection))
        .route("/proxy/connect", post(handler::handle_connection))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .with_state(state);

    let addr = format!("{}:{}", proxy_config.listen_addr, proxy_config.listen_port);
    tracing::info!("Database proxy listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
