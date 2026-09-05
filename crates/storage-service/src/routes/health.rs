use axum::Json;
use freebuff_shared::{ComponentHealth, HealthStatus};

pub async fn health_check() -> Json<HealthStatus> {
    Json(HealthStatus {
        status: "healthy".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        uptime_secs: 0,
        components: vec![ComponentHealth {
            name: "storage".into(),
            status: "healthy".into(),
            message: None,
        }],
    })
}
