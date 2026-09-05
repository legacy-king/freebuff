use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub redis_url: String,
    pub api_host: String,
    pub api_port: u16,
    pub jwt_secret: String,
    pub jwt_expiration_secs: i64,
    pub s3_endpoint: String,
    pub s3_bucket: String,
    pub s3_access_key: String,
    pub s3_secret_key: String,
    pub default_region: String,
    // Stripe billing
    pub stripe_secret_key: String,
    pub stripe_webhook_secret: String,
    pub stripe_price_id: String,
    pub stripe_meter_storage: String,
    pub stripe_meter_compute: String,
    pub stripe_meter_api_calls: String,
    // Internal service-to-service auth
    pub internal_api_token: String,
    // Database storage metering
    pub storage_sample_secs: u64,
    pub pg_monitor_user: String,
    pub pg_monitor_password: String,
}

impl Config {
    pub fn stripe_enabled(&self) -> bool {
        !self.stripe_secret_key.is_empty()
    }

    /// Stripe Billing Meter event name for a given usage meter.
    pub fn meter_event_name(&self, meter: crate::models::UsageMeter) -> String {
        match meter {
            crate::models::UsageMeter::StorageGb => self.stripe_meter_storage.clone(),
            crate::models::UsageMeter::ComputeHours => self.stripe_meter_compute.clone(),
            crate::models::UsageMeter::ApiCalls => self.stripe_meter_api_calls.clone(),
        }
    }

    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();
        Self {
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgres://freebuff:freebuff@localhost:5432/freebuff".into()),
            redis_url: env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://localhost:6379".into()),
            api_host: env::var("API_HOST").unwrap_or_else(|_| "0.0.0.0".into()),
            api_port: env::var("API_PORT")
                .unwrap_or_else(|_| "3000".into())
                .parse()
                .unwrap_or(3000),
            jwt_secret: env::var("JWT_SECRET")
                .unwrap_or_else(|_| "dev-secret-change-in-production".into()),
            jwt_expiration_secs: env::var("JWT_EXPIRATION_SECS")
                .unwrap_or_else(|_| "3600".into())
                .parse()
                .unwrap_or(3600),
            s3_endpoint: env::var("S3_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:9000".into()),
            s3_bucket: env::var("S3_BUCKET")
                .unwrap_or_else(|_| "freebuff".into()),
            s3_access_key: env::var("S3_ACCESS_KEY")
                .unwrap_or_else(|_| "minioadmin".into()),
            s3_secret_key: env::var("S3_SECRET_KEY")
                .unwrap_or_else(|_| "minioadmin".into()),
            default_region: env::var("DEFAULT_REGION")
                .unwrap_or_else(|_| "us-east-1".into()),
            stripe_secret_key: env::var("STRIPE_SECRET_KEY").unwrap_or_default(),
            stripe_webhook_secret: env::var("STRIPE_WEBHOOK_SECRET").unwrap_or_default(),
            stripe_price_id: env::var("STRIPE_PRICE_ID").unwrap_or_default(),
            stripe_meter_storage: env::var("STRIPE_METER_STORAGE")
                .unwrap_or_else(|_| "freebuff_storage_gb".into()),
            stripe_meter_compute: env::var("STRIPE_METER_COMPUTE")
                .unwrap_or_else(|_| "freebuff_compute_hours".into()),
            stripe_meter_api_calls: env::var("STRIPE_METER_API_CALLS")
                .unwrap_or_else(|_| "freebuff_api_calls".into()),
            internal_api_token: env::var("INTERNAL_API_TOKEN")
                .unwrap_or_else(|_| "dev-internal-token-change-me".into()),
            storage_sample_secs: env::var("STORAGE_SAMPLE_SECS")
                .unwrap_or_else(|_| "300".into())
                .parse()
                .unwrap_or(300),
            pg_monitor_user: env::var("PG_MONITOR_USER")
                .unwrap_or_else(|_| "freebuff".into()),
            pg_monitor_password: env::var("PG_MONITOR_PASSWORD")
                .unwrap_or_else(|_| "freebuff".into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub listen_addr: String,
    pub listen_port: u16,
    pub control_plane_url: String,
    pub max_connections_per_project: u32,
    pub connection_timeout_secs: u64,
    pub idle_timeout_secs: u64,
}

impl ProxyConfig {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();
        Self {
            listen_addr: env::var("PROXY_HOST")
                .unwrap_or_else(|_| "0.0.0.0".into()),
            listen_port: env::var("PROXY_PORT")
                .unwrap_or_else(|_| "5432".into())
                .parse()
                .unwrap_or(5432),
            control_plane_url: env::var("CONTROL_PLANE_URL")
                .unwrap_or_else(|_| "http://localhost:3001".into()),
            max_connections_per_project: env::var("MAX_CONNECTIONS_PER_PROJECT")
                .unwrap_or_else(|_| "100".into())
                .parse()
                .unwrap_or(100),
            connection_timeout_secs: env::var("CONNECTION_TIMEOUT_SECS")
                .unwrap_or_else(|_| "10".into())
                .parse()
                .unwrap_or(10),
            idle_timeout_secs: env::var("IDLE_TIMEOUT_SECS")
                .unwrap_or_else(|_| "600".into())
                .parse()
                .unwrap_or(600),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub listen_addr: String,
    pub listen_port: u16,
    pub control_plane_url: String,
    pub max_body_size: usize,
    pub rate_limit_per_second: u32,
    pub internal_api_token: String,
}

impl GatewayConfig {
    pub fn from_env() -> Self {
        dotenvy::dotenv().ok();
        Self {
            listen_addr: env::var("GATEWAY_HOST")
                .unwrap_or_else(|_| "0.0.0.0".into()),
            listen_port: env::var("GATEWAY_PORT")
                .unwrap_or_else(|_| "8000".into())
                .parse()
                .unwrap_or(8000),
            control_plane_url: env::var("CONTROL_PLANE_URL")
                .unwrap_or_else(|_| "http://localhost:3001".into()),
            max_body_size: env::var("MAX_BODY_SIZE")
                .unwrap_or_else(|_| "10485760".into()) // 10MB
                .parse()
                .unwrap_or(10_485_760),
            rate_limit_per_second: env::var("RATE_LIMIT_PER_SECOND")
                .unwrap_or_else(|_| "100".into())
                .parse()
                .unwrap_or(100),
            internal_api_token: env::var("INTERNAL_API_TOKEN")
                .unwrap_or_else(|_| "dev-internal-token-change-me".into()),
        }
    }
}
