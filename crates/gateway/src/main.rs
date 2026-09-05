mod routes;
mod middleware;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::{routing::{get, post}, Router};
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
pub struct GatewayState {
    pub config: freebuff_shared::GatewayConfig,
    /// In-memory usage counters, batched and flushed to the control plane.
    pub usage_counts: Arc<Mutex<HashMap<String, u64>>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "freebuff_gateway=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = freebuff_shared::GatewayConfig::from_env();

    let state = GatewayState {
        config: config.clone(),
        usage_counts: Arc::new(Mutex::new(HashMap::new())),
    };

    // Report accumulated API-call usage to the control plane every 60s.
    tokio::spawn(usage_reporter_loop(state.clone()));

    let app = Router::new()
        .route("/health", get(routes::health::health_check))
        .route("/rest/v1/:table", get(routes::rest::list_rows))
        .route("/rest/v1/:table", post(routes::rest::insert_rows))
        .route("/rest/v1/:table", axum::routing::patch(routes::rest::update_rows))
        .route("/rest/v1/:table", axum::routing::delete(routes::rest::delete_rows))
        .route("/realtime/v1/websocket", get(routes::realtime::websocket_handler))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::auth_middleware,
        ))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("{}:{}", config.listen_addr, config.listen_port);
    tracing::info!("API Gateway listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Drain the in-memory usage counters and POST them to the control plane's
/// internal usage endpoint. At-most-once delivery: on failure the batch is
/// dropped and metering continues with the next interval.
async fn usage_reporter_loop(state: GatewayState) {
    let http = reqwest::Client::new();
    let endpoint = format!("{}/v1/internal/usage", state.config.control_plane_url);
    let mut interval = tokio::time::interval(Duration::from_secs(60));

    loop {
        interval.tick().await;

        let counts = {
            let mut guard = state.usage_counts.lock().await;
            std::mem::take(&mut *guard)
        };

        if counts.is_empty() {
            continue;
        }

        let events: Vec<serde_json::Value> = counts
            .into_iter()
            .map(|(meter, value)| serde_json::json!({ "meter": meter, "value": value }))
            .collect();
        let body = serde_json::json!({ "events": events });

        match http
            .post(&endpoint)
            .bearer_auth(&state.config.internal_api_token)
            .json(&body)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                tracing::debug!("Reported {} usage events to control plane", events.len());
            }
            Ok(response) => {
                tracing::warn!(
                    "Usage report to control plane failed with status {}",
                    response.status()
                );
            }
            Err(e) => {
                tracing::warn!("Usage report to control plane failed: {}", e);
            }
        }
    }
}