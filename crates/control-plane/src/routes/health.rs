use axum::Json;
use freebuff_shared::{ComponentHealth, HealthStatus};

pub async fn health_check() -> Json<HealthStatus> {
    Json(HealthStatus {
        status: "healthy".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        uptime_secs: {
            static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
            START.get_or_init(std::time::Instant::now).elapsed().as_secs()
        },
        components: vec![
            ComponentHealth {
                name: "control-plane".into(),
                status: "healthy".into(),
                message: None,
            },
        ],
    })
}
