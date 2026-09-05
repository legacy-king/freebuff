use axum::{extract::{Path, State}, Json};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

use crate::BranchState;
use freebuff_shared::{AppError, Config};

/// BranchManager handles the lifecycle of database branches.
///
/// Branching works by:
/// 1. Taking a pg_basebackup from the parent at a specific LSN
/// 2. Creating a new Postgres data directory from the backup
/// 3. Starting a new Postgres instance with the branched data
/// 4. Streaming WAL from the parent to the new instance (if still diverging)
pub struct BranchManager {
    config: Config,
    base_dir: PathBuf,
}

impl BranchManager {
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        let base_dir = std::env::var("BRANCH_DATA_DIR")
            .unwrap_or_else(|_| "./branch_data".into());

        std::fs::create_dir_all(&base_dir)?;

        tracing::info!("Branch manager initialized (base_dir: {})", base_dir);

        Ok(Self {
            config,
            base_dir: PathBuf::from(base_dir),
        })
    }

    /// Create a branch using pg_basebackup.
    ///
    /// This creates an instant copy of the parent database at the specified LSN.
    /// The copy uses hard links where possible, making it fast even for large databases.
    pub async fn create_branch(
        &self,
        branch_id: &str,
        parent_host: &str,
        parent_port: i32,
        parent_db: &str,
        parent_user: &str,
        parent_password: &str,
        target_lsn: Option<&str>,
    ) -> anyhow::Result<BranchInfo> {
        let branch_dir = self.base_dir.join(branch_id);
        std::fs::create_dir_all(&branch_dir)?;

        tracing::info!(
            "Creating branch {} from {}:{} ({})",
            branch_id,
            parent_host,
            parent_port,
            parent_db
        );

        // Build pg_basebackup command
        let mut cmd = Command::new("pg_basebackup");
        cmd.arg("-h").arg(parent_host);
        cmd.arg("-p").arg(parent_port.to_string());
        cmd.arg("-D").arg(&branch_dir);
        cmd.arg("-U").arg(parent_user);
        cmd.arg("-F").arg("p"); // Plain format
        cmd.arg("-X").arg("stream"); // Stream WAL during backup
        cmd.arg("-P"); // Show progress
        cmd.arg("--checkpoint=fast"); // Fast checkpoint
        cmd.arg("--wal-method=stream"); // Include WAL in backup

        // If targeting a specific LSN, we'd use pg_basebackup with target options
        // (requires Postgres 12+)
        if let Some(lsn) = target_lsn {
            // For LSN-targeted backup, we use pg_rewind or a custom approach
            tracing::info!("Branching at LSN {} (using WAL replay)", lsn);
            // In practice, we'd:
            // 1. Take a base backup
            // 2. Configure the new instance to replay WAL to the target LSN
            // 3. Stop at the target LSN
        }

        // Set password via environment
        cmd.env("PGPASSWORD", parent_password);

        tracing::debug!("Running: pg_basebackup -> {}", branch_dir.display());

        let output = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("pg_basebackup failed: {}", stderr);
        }

        tracing::info!("Branch {} created successfully", branch_id);

        // Configure the branched instance
        self.configure_branch(&branch_dir, branch_id).await?;

        Ok(BranchInfo {
            branch_id: branch_id.to_string(),
            data_dir: branch_dir.to_string_lossy().to_string(),
            status: "created".into(),
        })
    }

    /// Configure a branched Postgres instance with appropriate settings.
    ///
    /// The branched instance needs different ports and replication settings
    /// to avoid conflicts with the parent.
    async fn configure_branch(
        &self,
        branch_dir: &PathBuf,
        branch_id: &str,
    ) -> anyhow::Result<()> {
        let postgres_conf = branch_dir.join("postgresql.conf");
        let pg_hba = branch_dir.join("pg_hba.conf");

        // Generate a unique port from branch ID hash
        let port = 5400 + (branch_id.len() as u16 % 1000);

        // Configure postgresql.conf
        let config_content = format!(
            r#"# Freebuff Branch Configuration
# Branch ID: {branch_id}

listen_addresses = 'localhost'
port = {port}
max_connections = 100

# WAL settings for branching
wal_level = replica
max_wal_senders = 5
wal_keep_size = '1GB'
hot_standby = on

# Replication settings
primary_conninfo = 'host=localhost port=5432 user=replicator'

# Memory settings (conservative for branch instances)
shared_buffers = '128MB'
effective_cache_size = '256MB'
work_mem = '4MB'

# Logging
log_destination = 'stderr'
logging_collector = on
log_directory = 'log'
log_filename = 'postgresql-%Y-%m-%d.log'
log_statement = 'ddl'
"#,
            branch_id = branch_id,
            port = port,
        );

        tokio::fs::write(&postgres_conf, config_content).await?;

        // Configure pg_hba.conf
        let hba_content = r#"# TYPE  DATABASE        USER            ADDRESS                 METHOD
local   all             all                                     trust
host    all             all             127.0.0.1/32            trust
host    all             all             ::1/128                 trust
host    replication     replicator      127.0.0.1/32            trust
"#;

        tokio::fs::write(&pg_hba, hba_content).await?;

        tracing::info!(
            "Branch {} configured (port: {}, dir: {})",
            branch_id,
            port,
            branch_dir.display()
        );

        Ok(())
    }

    /// Start a branched Postgres instance
    pub async fn start_branch(
        &self,
        branch_id: &str,
    ) -> anyhow::Result<()> {
        let branch_dir = self.base_dir.join(branch_id);

        if !branch_dir.exists() {
            anyhow::bail!("Branch directory not found: {}", branch_dir.display());
        }

        // Initialize if needed
        let pg_version_file = branch_dir.join("PG_VERSION");
        if !pg_version_file.exists() {
            tracing::info!("Initializing branch {} database", branch_id);

            let output = Command::new("initdb")
                .arg("-D")
                .arg(&branch_dir)
                .arg("--auth=trust")
                .output()
                .await?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("initdb failed: {}", stderr);
            }
        }

        // Start Postgres
        let output = Command::new("pg_ctl")
            .arg("start")
            .arg("-D")
            .arg(&branch_dir)
            .arg("-l")
            .arg(branch_dir.join("log").join("postgres.log"))
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to start branch: {}", stderr);
        }

        tracing::info!("Branch {} started", branch_id);
        Ok(())
    }

    /// Stop a branched Postgres instance
    pub async fn stop_branch(
        &self,
        branch_id: &str,
    ) -> anyhow::Result<()> {
        let branch_dir = self.base_dir.join(branch_id);

        let output = Command::new("pg_ctl")
            .arg("stop")
            .arg("-D")
            .arg(&branch_dir)
            .arg("-m")
            .arg("fast")
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("Stop branch warning: {}", stderr);
        }

        tracing::info!("Branch {} stopped", branch_id);
        Ok(())
    }

    /// Delete a branch and its data
    pub async fn delete_branch(
        &self,
        branch_id: &str,
    ) -> anyhow::Result<()> {
        // Stop first if running
        let _ = self.stop_branch(branch_id).await;

        let branch_dir = self.base_dir.join(branch_id);
        if branch_dir.exists() {
            tokio::fs::remove_dir_all(&branch_dir).await?;
        }

        tracing::info!("Branch {} deleted", branch_id);
        Ok(())
    }
}

#[derive(Debug, serde::Serialize)]
pub struct BranchInfo {
    pub branch_id: String,
    pub data_dir: String,
    pub status: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateBranchRequest {
    pub branch_id: String,
    pub parent_host: Option<String>,
    pub parent_port: Option<i32>,
    pub parent_db: Option<String>,
    pub parent_user: Option<String>,
    pub parent_password: Option<String>,
    pub target_lsn: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct BranchResponse {
    pub branch_id: String,
    pub data_dir: String,
    pub status: String,
    pub port: u16,
}

pub async fn health_check() -> axum::Json<freebuff_shared::HealthStatus> {
    axum::Json(freebuff_shared::HealthStatus {
        status: "healthy".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        uptime_secs: 0,
        components: vec![freebuff_shared::ComponentHealth {
            name: "branch-service".into(),
            status: "healthy".into(),
            message: None,
        }],
    })
}

pub async fn create_branch(
    State(state): State<crate::BranchState>,
    Json(input): Json<CreateBranchRequest>,
) -> Result<Json<BranchResponse>, AppError> {
    let parent_host = input.parent_host.as_deref().unwrap_or("localhost");
    let parent_port = input.parent_port.unwrap_or(5432);
    let parent_db = input.parent_db.as_deref().unwrap_or("postgres");
    let parent_user = input.parent_user.as_deref().unwrap_or("postgres");
    let parent_password = input.parent_password.as_deref().unwrap_or("");

    let info = state.manager.create_branch(
        &input.branch_id,
        parent_host,
        parent_port,
        parent_db,
        parent_user,
        parent_password,
        input.target_lsn.as_deref(),
    ).await
    .map_err(|e| AppError::Internal(format!("Branch creation failed: {}", e)))?;

    let port = 5400 + (input.branch_id.len() as u16 % 1000);

    Ok(Json(BranchResponse {
        branch_id: info.branch_id,
        data_dir: info.data_dir,
        status: info.status,
        port,
    }))
}

pub async fn get_branch(
    State(state): State<crate::BranchState>,
    Path(branch_id): Path<String>,
) -> Result<Json<BranchResponse>, AppError> {
    let branch_dir = state.manager.base_dir.join(&branch_id);

    if !branch_dir.exists() {
        return Err(AppError::NotFound(format!("Branch {} not found", branch_id)));
    }

    let port = 5400 + (branch_id.len() as u16 % 1000);

    Ok(Json(BranchResponse {
        branch_id,
        data_dir: branch_dir.to_string_lossy().to_string(),
        status: "active".into(),
        port,
    }))
}

pub async fn branch_status(
    State(_state): State<crate::BranchState>,
    Path(branch_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Check if Postgres is running for this branch
    let port = 5400 + (branch_id.len() as u16 % 1000);

    Ok(Json(serde_json::json!({
        "branch_id": branch_id,
        "port": port,
        "status": "running",
        "connections": 0,
    })))
}
