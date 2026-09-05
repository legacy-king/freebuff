use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

// ── Organization ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateOrganization {
    pub name: String,
    pub slug: Option<String>,
}

// ── Project ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Project {
    pub id: Uuid,
    pub org_id: Uuid,
    pub name: String,
    pub slug: String,
    pub region: String,
    pub status: ProjectStatus,
    pub database_host: Option<String>,
    pub database_port: Option<i32>,
    pub database_name: Option<String>,
    pub api_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "project_status", rename_all = "snake_case")]
pub enum ProjectStatus {
    Creating,
    Active,
    Suspended,
    Deleting,
    Deleted,
    Failed,
}

impl std::fmt::Display for ProjectStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Creating => write!(f, "creating"),
            Self::Active => write!(f, "active"),
            Self::Suspended => write!(f, "suspended"),
            Self::Deleting => write!(f, "deleting"),
            Self::Deleted => write!(f, "deleted"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateProject {
    pub name: String,
    pub slug: Option<String>,
    pub region: Option<String>,
    pub plan: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProject {
    pub name: Option<String>,
}

// ── Branch ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Branch {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub slug: String,
    pub parent_branch_id: Option<Uuid>,
    pub parent_lsn: Option<i64>,
    pub status: BranchStatus,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "branch_status", rename_all = "snake_case")]
pub enum BranchStatus {
    Creating,
    Active,
    Inactive,
    Deleting,
    Failed,
}

#[derive(Debug, Deserialize)]
pub struct CreateBranch {
    pub name: String,
    pub slug: Option<String>,
    pub parent_branch_id: Option<Uuid>,
    pub parent_lsn: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateBranch {
    pub name: Option<String>,
}

// ── Compute Endpoint ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ComputeEndpoint {
    pub id: Uuid,
    pub branch_id: Uuid,
    pub project_id: Uuid,
    pub host: String,
    pub port: i32,
    pub status: ComputeStatus,
    pub compute_size: ComputeSize,
    pub max_connections: i32,
    pub last_accrued_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "compute_status", rename_all = "snake_case")]
pub enum ComputeStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "compute_size", rename_all = "snake_case")]
pub enum ComputeSize {
    Micro,
    Small,
    Medium,
    Large,
}

impl ComputeSize {
    pub fn max_connections(&self) -> i32 {
        match self {
            Self::Micro => 20,
            Self::Small => 100,
            Self::Medium => 500,
            Self::Large => 2000,
        }
    }

    pub fn memory_mb(&self) -> i32 {
        match self {
            Self::Micro => 256,
            Self::Small => 1024,
            Self::Medium => 4096,
            Self::Large => 16384,
        }
    }

    /// Multiplier applied to wall-clock time to meter compute hours
    /// (a `micro` endpoint accrues 0.25h per running hour, `large` 16h).
    pub fn compute_factor(&self) -> f64 {
        match self {
            Self::Micro => 0.25,
            Self::Small => 1.0,
            Self::Medium => 4.0,
            Self::Large => 16.0,
        }
    }
}

// ── Role ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Role {
    pub id: Uuid,
    pub branch_id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub is_superuser: bool,
    pub can_login: bool,
    pub created_at: DateTime<Utc>,
}

// ── API Key ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKey {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub key_hash: String,
    pub key_prefix: String,
    pub key_type: ApiKeyType,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "api_key_type", rename_all = "snake_case")]
pub enum ApiKeyType {
    Publishable,
    Secret,
}

#[derive(Debug, Deserialize)]
pub struct CreateApiKey {
    pub name: String,
    pub key_type: ApiKeyType,
    pub scopes: Option<Vec<String>>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct CreateApiKeyResponse {
    pub id: Uuid,
    pub name: String,
    pub key: String,
    pub key_prefix: String,
    pub key_type: ApiKeyType,
    pub scopes: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

// ── User ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
    pub password_hash: String,
    pub email_verified: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateUser {
    pub email: String,
    pub password: String,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LoginUser {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub user: UserPublic,
}

#[derive(Debug, Serialize)]
pub struct UserPublic {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ── Connection Info ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub host: String,
    pub port: i32,
    pub database: String,
    pub role: String,
    pub password: String,
    pub ssl_mode: String,
}

impl ConnectionInfo {
    pub fn connection_string(&self) -> String {
        format!(
            "postgresql://{}:{}@{}:{}/{}?sslmode={}",
            self.role, self.password, self.host, self.port, self.database, self.ssl_mode
        )
    }

    pub fn pooler_connection_string(&self) -> String {
        format!(
            "postgresql://{}:{}@{}:{}/{}?sslmode={}",
            self.role, self.password, self.host, self.port, self.database, self.ssl_mode
        )
    }
}

// ── Billing ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "usage_meter", rename_all = "snake_case")]
pub enum UsageMeter {
    StorageGb,
    ComputeHours,
    ApiCalls,
}

impl std::fmt::Display for UsageMeter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::StorageGb => "storage_gb",
            Self::ComputeHours => "compute_hours",
            Self::ApiCalls => "api_calls",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BillingAccount {
    pub id: Uuid,
    pub org_id: Uuid,
    pub stripe_customer_id: Option<String>,
    pub stripe_subscription_id: Option<String>,
    pub plan: String,
    pub status: String,
    pub billing_email: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct BillingAccountPublic {
    pub id: Uuid,
    pub org_id: Uuid,
    pub plan: String,
    pub status: String,
    pub billing_email: Option<String>,
    pub has_subscription: bool,
    pub created_at: DateTime<Utc>,
}

impl From<BillingAccount> for BillingAccountPublic {
    fn from(account: BillingAccount) -> Self {
        Self {
            has_subscription: account
                .stripe_subscription_id
                .as_ref()
                .map_or(false, |s| !s.is_empty()),
            plan: account.plan,
            status: account.status,
            billing_email: account.billing_email,
            id: account.id,
            org_id: account.org_id,
            created_at: account.created_at,
        }
    }
}

/// One raw metering event submitted by any service via the internal endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct InternalUsageEvent {
    pub org_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub meter: UsageMeter,
    pub value: f64,
    pub occurred_at: Option<DateTime<Utc>>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct InternalUsageIngest {
    pub events: Vec<InternalUsageEvent>,
}

#[derive(Debug, Deserialize)]
pub struct StripeSessionRequest {
    pub success_url: Option<String>,
    pub cancel_url: Option<String>,
    pub return_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StripeSessionResponse {
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct DailyUsage {
    pub date: chrono::NaiveDate,
    pub value: f64,
}

#[derive(Debug, Serialize)]
pub struct MeterUsage {
    pub meter: UsageMeter,
    pub total: f64,
    pub daily: Vec<DailyUsage>,
}

#[derive(Debug, Serialize)]
pub struct UsageSummary {
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub meters: Vec<MeterUsage>,
}

// ── Health ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub version: String,
    pub uptime_secs: u64,
    pub components: Vec<ComponentHealth>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ComponentHealth {
    pub name: String,
    pub status: String,
    pub message: Option<String>,
}

// ── Pagination ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    pub page: Option<u32>,
    pub per_page: Option<u32>,
}

impl PaginationParams {
    pub fn offset(&self) -> u32 {
        let page = self.page.unwrap_or(1).max(1);
        let per_page = self.per_page.unwrap_or(20).min(100);
        (page - 1) * per_page
    }

    pub fn limit(&self) -> u32 {
        self.per_page.unwrap_or(20).min(100)
    }
}

#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub data: Vec<T>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
}
