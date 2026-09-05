mod logical_replication;
mod change_event;
mod subscription;
mod websocket;
mod presence;
mod broadcast;
mod protocol;

use axum::{routing::{get, post}, Router};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use std::collections::HashMap;

pub use change_event::{ChangeEvent, ChangeOperation, ColumnValue, SubscriptionFilter};
pub use subscription::Subscription;

#[derive(Clone)]
pub struct CdcState {
    /// Active WebSocket connections grouped by project
    pub connections: Arc<RwLock<HashMap<String, Vec<Arc<websocket::WsConnection>>>>>,
    /// Active subscriptions grouped by project_id.table_name
    pub subscriptions: Arc<RwLock<HashMap<String, Vec<Subscription>>>>,
    /// Presence tracking
    pub presence: Arc<presence::PresenceManager>,
    /// Broadcast channels
    pub broadcast: Arc<broadcast::BroadcastManager>,
    /// Configuration
    pub config: CdcConfig,
    /// Logical replication clients per project
    pub replication_clients: Arc<RwLock<HashMap<String, logical_replication::ReplicationClient>>>,
}

#[derive(Debug, Clone)]
pub struct CdcConfig {
    pub listen_addr: String,
    pub listen_port: u16,
    pub database_url: String,
    pub max_connections_per_project: u32,
    pub heartbeat_interval_secs: u64,
    pub max_subscriptions_per_connection: u32,
}

impl CdcConfig {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();
        Self {
            listen_addr: std::env::var("CDC_HOST")
                .unwrap_or_else(|_| "0.0.0.0".into()),
            listen_port: std::env::var("CDC_PORT")
                .unwrap_or_else(|_| "5003".into())
                .parse()
                .unwrap_or(5003),
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://freebuff:freebuff@localhost:5432/freebuff".into()),
            max_connections_per_project: std::env::var("CDC_MAX_CONNECTIONS")
                .unwrap_or_else(|_| "1000".into())
                .parse()
                .unwrap_or(1000),
            heartbeat_interval_secs: std::env::var("CDC_HEARTBEAT_SECS")
                .unwrap_or_else(|_| "30".into())
                .parse()
                .unwrap_or(30),
            max_subscriptions_per_connection: std::env::var("CDC_MAX_SUBS_PER_CONN")
                .unwrap_or_else(|_| "100".into())
                .parse()
                .unwrap_or(100),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "freebuff_cdc_service=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = CdcConfig::from_env();

    let state = CdcState {
        connections: Arc::new(RwLock::new(HashMap::new())),
        subscriptions: Arc::new(RwLock::new(HashMap::new())),
        presence: Arc::new(presence::PresenceManager::new()),
        broadcast: Arc::new(broadcast::BroadcastManager::new()),
        config: config.clone(),
        replication_clients: Arc::new(RwLock::new(HashMap::new())),
    };

    // Start the heartbeat task
    let heartbeat_state = state.clone();
    tokio::spawn(async move {
        heartbeat_loop(heartbeat_state).await;
    });

    let app = Router::new()
        // Health
        .route("/health", get(health_check))
        // WebSocket endpoint for real-time subscriptions
        .route("/realtime/v1/websocket", get(websocket::ws_handler))
        // REST API for subscriptions
        .route("/realtime/v1/channels", post(subscription::create_subscription))
        .route("/realtime/v1/channels/:channel", delete(subscription::remove_subscription))
        .route("/realtime/v1/channels/:channel/broadcast", post(broadcast::send_broadcast))
        // Presence API
        .route("/realtime/v1/presence/:channel", get(presence::get_presence))
        .route("/realtime/v1/presence/:channel/join", post(presence::join_presence))
        .route("/realtime/v1/presence/:channel/leave", post(presence::leave_presence))
        // CDC status
        .route("/v1/status", get(cdc_status))
        .route("/v1/projects/:project_id/replication", post(start_replication))
        // Middleware
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("{}:{}", config.listen_addr, config.listen_port);
    tracing::info!("CDC service listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
        "service": "cdc-service"
    }))
}

async fn cdc_status(
    axum::extract::State(state): axum::extract::State<CdcState>,
) -> axum::Json<serde_json::Value> {
    let connections = state.connections.read().await;
    let subscriptions = state.subscriptions.read().await;
    let replication = state.replication_clients.read().await;

    let total_connections: usize = connections.values().map(|c| c.len()).sum();
    let total_subscriptions: usize = subscriptions.values().map(|s| s.len()).sum();

    axum::Json(serde_json::json!({
        "status": "running",
        "connections": total_connections,
        "subscriptions": total_subscriptions,
        "projects_with_replication": replication.len(),
    }))
}

async fn start_replication(
    axum::extract::Path(project_id): axum::extract::Path<String>,
    axum::extract::State(state): axum::extract::State<CdcState>,
) -> Result<axum::Json<serde_json::Value>, freebuff_shared::AppError> {
    // Start logical replication for a project
    let config = state.config.clone();
    let project_id_clone = project_id.clone();

    let mut clients = state.replication_clients.write().await;

    if clients.contains_key(&project_id) {
        return Ok(axum::Json(serde_json::json!({
            "status": "already_running",
            "project_id": project_id,
        })));
    }

    let client = logical_replication::ReplicationClient::new(
        &config.database_url,
        &project_id,
    ).await
    .map_err(|e| freebuff_shared::AppError::Internal(format!("Failed to create replication client: {}", e)))?;

    clients.insert(project_id.clone(), client);

    tracing::info!("Started replication for project {}", project_id);

    Ok(axum::Json(serde_json::json!({
        "status": "started",
        "project_id": project_id,
    })))
}

async fn heartbeat_loop(state: CdcState) {
    let interval = std::time::Duration::from_secs(state.config.heartbeat_interval_secs);

    loop {
        tokio::time::sleep(interval).await;

        let connections = state.connections.read().await;
        for (project_id, conns) in connections.iter() {
            for conn in conns {
                let _ = conn.send_heartbeat().await;
            }
        }
    }
}
