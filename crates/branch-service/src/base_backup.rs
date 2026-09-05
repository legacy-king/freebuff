use axum::{extract::{Path, State}, Json};
use std::path::PathBuf;
use tokio::process::Command;

use crate::BranchState;
use freebuff_shared::AppError;

#[derive(Debug, serde::Serialize)]
pub struct BackupInfo {
    pub backup_id: String,
    pub branch_id: String,
    pub lsn: String,
    pub size_bytes: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateBackupRequest {
    pub lsn: Option<String>,
}

/// Create a base backup of a branch.
///
/// This creates a compressed tar archive of the Postgres data directory,
/// which can be used to restore the branch to this point in time.
pub async fn create_backup(
    State(state): State<BranchState>,
    Path(branch_id): Path<String>,
    Json(input): Json<CreateBackupRequest>,
) -> Result<Json<BackupInfo>, AppError> {
    let branch_dir = std::path::PathBuf::from(&state.config.s3_endpoint)
        .join("branch_data")
        .join(&branch_id);

    if !branch_dir.exists() {
        return Err(AppError::NotFound(format!("Branch {} not found", branch_id)));
    }

    let backup_id = uuid::Uuid::new_v4().to_string();
    let backup_dir = std::path::PathBuf::from("./backups").join(&branch_id);
    std::fs::create_dir_all(&backup_dir)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    let backup_file = backup_dir.join(format!("{}.tar.zst", backup_id));

    tracing::info!(
        "Creating backup {} for branch {}",
        backup_id,
        branch_id
    );

    // Create compressed tar archive of the data directory
    // Using pg_basebackup's format or a custom tar approach
    let output = Command::new("tar")
        .arg("-cf")
        .arg(&backup_file)
        .arg("--use-compress-program=zstd")
        .arg("-C")
        .arg(&branch_dir)
        .arg(".")
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to run tar: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Internal(format!("Backup failed: {}", stderr)));
    }

    let size = std::fs::metadata(&backup_file)
        .map(|m| m.len())
        .unwrap_or(0);

    // Get current LSN from the branch
    let lsn = input.lsn.unwrap_or_else(|| "0/0".into());

    tracing::info!(
        "Backup {} created ({} bytes) for branch {}",
        backup_id,
        size,
        branch_id
    );

    Ok(Json(BackupInfo {
        backup_id,
        branch_id,
        lsn,
        size_bytes: size,
        created_at: chrono::Utc::now(),
    }))
}

/// Restore a branch from a base backup.
///
/// This extracts a compressed tar archive to recreate the Postgres data directory.
pub async fn restore_backup(
    State(_state): State<BranchState>,
    Path(branch_id): Path<String>,
    Json(input): Json<RestoreBackupRequest>,
) -> Result<Json<RestoreInfo>, AppError> {
    let backup_file = std::path::PathBuf::from("./backups")
        .join(&branch_id)
        .join(format!("{}.tar.zst", input.backup_id));

    if !backup_file.exists() {
        return Err(AppError::NotFound(format!("Backup {} not found", input.backup_id)));
    }

    let restore_dir = std::path::PathBuf::from("./branch_data")
        .join(&branch_id);

    // Clear existing data (with safety check)
    if restore_dir.exists() && input.force.unwrap_or(false) {
        tokio::fs::remove_dir_all(&restore_dir).await
            .map_err(|e| AppError::Internal(e.to_string()))?;
    }

    std::fs::create_dir_all(&restore_dir)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    tracing::info!(
        "Restoring backup {} to branch {}",
        input.backup_id,
        branch_id
    );

    // Extract the archive
    let output = Command::new("tar")
        .arg("-xf")
        .arg(&backup_file)
        .arg("--use-compress-program=zstd")
        .arg("-C")
        .arg(&restore_dir)
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to restore: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Internal(format!("Restore failed: {}", stderr)));
    }

    tracing::info!("Branch {} restored from backup {}", branch_id, input.backup_id);

    Ok(Json(RestoreInfo {
        branch_id,
        backup_id: input.backup_id,
        status: "restored".into(),
    }))
}

#[derive(Debug, serde::Deserialize)]
pub struct RestoreBackupRequest {
    pub backup_id: String,
    pub force: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
pub struct RestoreInfo {
    pub branch_id: String,
    pub backup_id: String,
    pub status: String,
}
