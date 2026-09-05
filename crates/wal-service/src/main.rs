mod wal_receiver;
mod wal_archiver;
mod wal_server;
mod timeline;

use axum::{routing::{get, post}, Router};
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use std::collections::HashMap;

pub use timeline::{Timeline, TimelineState, LSN};

#[derive(Clone)]
pub struct WalState {
    pub timelines: Arc<RwLock<HashMap<String, Arc<Timeline>>>>,
    pub config: WalConfig,
    pub archiver: Arc<wal_archiver::WalArchiver>,
}

#[derive(Debug, Clone)]
pub struct WalConfig {
    pub listen_addr: String,
    pub listen_port: u16,
    pub wal_dir: String,
    pub s3_endpoint: String,
    pub s3_bucket: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub archive_interval_secs: u64,
}

impl WalConfig {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();
        Self {
            listen_addr: std::env::var("WAL_HOST")
                .unwrap_or_else(|_| "0.0.0.0".into()),
            listen_port: std::env::var("WAL_PORT")
                .unwrap_or_else(|_| "5001".into())
                .parse()
                .unwrap_or(5001),
            wal_dir: std::env::var("WAL_DIR")
                .unwrap_or_else(|_| "./wal_data".into()),
            s3_endpoint: std::env::var("S3_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:9000".into()),
            s3_bucket: std::env::var("S3_BUCKET")
                .unwrap_or_else(|_| "freebuff-wal".into()),
            s3_access_key: std::env::var("S3_ACCESS_KEY")
                .unwrap_or_else(|_| "minioadmin".into()),
            s3_secret_key: std::env::var("S3_SECRET_KEY")
                .unwrap_or_else(|_| "minioadmin".into()),
            archive_interval_secs: std::env::var("ARCHIVE_INTERVAL_SECS")
                .unwrap_or_else(|_| "60".into())
                .parse()
                .unwrap_or(60),
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "freebuff_wal_service=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = WalConfig::from_env();

    // Ensure WAL directory exists
    std::fs::create_dir_all(&config.wal_dir)?;

    let archiver = Arc::new(wal_archiver::WalArchiver::new(config.clone()).await?);

    let state = WalState {
        timelines: Arc::new(RwLock::new(HashMap::new())),
        config: config.clone(),
        archiver,
    };

    let app = Router::new()
        // Health
        .route("/health", get(wal_server::health_check))
        // Timeline management
        .route("/v1/timelines", post(wal_server::create_timeline))
        .route("/v1/timelines/:timeline_id", get(wal_server::get_timeline))
        .route("/v1/timelines/:timeline_id/status", get(wal_server::timeline_status))
        // WAL reception
        .route("/v1/timelines/:timeline_id/wal", post(wal_receiver::receive_wal))
        // WAL serving (for replicas and branches)
        .route("/v1/timelines/:timeline_id/wal/:start_lsn", get(wal_server::serve_wal))
        // PITR
        .route("/v1/timelines/:timeline_id/restore", post(wal_server::restore_pitr))
        // Branching
        .route("/v1/timelines/:timeline_id/branch", post(wal_server::create_branch_timeline))
        // Archival
        .route("/v1/timelines/:timeline_id/archive", post(wal_archiver::trigger_archive))
        // Middleware
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("{}:{}", config.listen_addr, config.listen_port);
    tracing::info!("WAL service listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
