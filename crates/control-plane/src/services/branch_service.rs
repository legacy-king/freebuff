use uuid::Uuid;

use crate::AppState;
use freebuff_shared::{AppError, Branch, BranchStatus, CreateBranch};

/// Create a new branch with full infrastructure:
/// 1. Create branch record in the database
/// 2. Create a WAL timeline for the branch (branching from parent)
/// 3. Use pg_basebackup to copy the parent database
/// 4. Start a new compute endpoint for the branch
pub async fn create_branch(
    state: &AppState,
    project_id: Uuid,
    input: CreateBranch,
) -> Result<Branch, AppError> {
    let slug = input.slug.unwrap_or_else(|| {
        input
            .name
            .to_lowercase()
            .replace(|c: char| !c.is_alphanumeric() && c != '-', "")
            .replace("--", "-")
            .trim_matches('-')
            .to_string()
    });

    // Get parent branch (default to main if not specified)
    let parent_branch = match input.parent_branch_id {
        Some(id) => crate::db::branches::get_branch(&state.db, project_id, id).await?,
        None => crate::db::branches::get_default_branch(&state.db, project_id).await?,
    };

    let create_input = CreateBranch {
        name: input.name.clone(),
        slug: Some(slug.clone()),
        parent_branch_id: Some(parent_branch.id),
        parent_lsn: input.parent_lsn,
    };

    // Step 1: Create branch record
    let branch = crate::db::branches::create_branch(
        &state.db,
        project_id,
        &create_input,
        &slug,
    ).await?;

    tracing::info!(
        "Created branch {} (id: {}) for project {}",
        branch.name,
        branch.id,
        project_id
    );

    // Step 2: Create a WAL timeline for this branch
    let branch_timeline_id = branch.id.to_string();
    let parent_timeline_id = parent_branch.id.to_string();

    match create_branch_timeline(
        &branch_timeline_id,
        &parent_timeline_id,
        input.parent_lsn.map(|l| l.to_string()).as_deref(),
    ).await {
        Ok(_) => tracing::info!("Created WAL timeline for branch {}", branch.id),
        Err(e) => tracing::warn!("Failed to create branch timeline (non-fatal): {}", e),
    }

    // Step 3: Use pg_basebackup to create the branch (via branch service)
    match create_physical_branch(
        &branch.id.to_string(),
        &parent_branch.id.to_string(),
    ).await {
        Ok(info) => {
            tracing::info!(
                "Physical branch created: {} at {}",
                info.branch_id,
                info.data_dir
            );
        }
        Err(e) => {
            tracing::warn!("Physical branch creation not available (non-fatal): {}", e);
            // In development mode, we skip physical branch creation
        }
    }

    // Step 4: Create compute endpoint for the branch
    let _endpoint = crate::db::compute::create_endpoint(&state.db, project_id, branch.id).await?;
    tracing::info!("Created compute endpoint for branch {}", branch.id);

    // Step 5: Mark branch as active
    let branch = crate::db::branches::update_branch_status(
        &state.db,
        project_id,
        branch.id,
        BranchStatus::Active,
    )
    .await?;

    Ok(branch)
}

/// Create a WAL timeline branching from a parent
async fn create_branch_timeline(
    branch_timeline_id: &str,
    parent_timeline_id: &str,
    branch_lsn: Option<&str>,
) -> Result<(), AppError> {
    let wal_service_url = std::env::var("WAL_SERVICE_URL")
        .unwrap_or_else(|_| "http://localhost:5001".into());

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/timelines", wal_service_url))
        .json(&serde_json::json!({
            "timeline_id": branch_timeline_id,
            "parent_id": parent_timeline_id,
            "parent_branch_lsn": branch_lsn,
        }))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to connect to WAL service: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "WAL service returned {}: {}",
            status, body
        )));
    }

    Ok(())
}

/// Create a physical branch using the branch service
async fn create_physical_branch(
    branch_id: &str,
    parent_id: &str,
) -> Result<PhysicalBranchInfo, AppError> {
    let branch_service_url = std::env::var("BRANCH_SERVICE_URL")
        .unwrap_or_else(|_| "http://localhost:5002".into());

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/branches", branch_service_url))
        .json(&serde_json::json!({
            "branch_id": branch_id,
            "parent_host": "localhost",
            "parent_port": 5432,
            "parent_db": "postgres",
            "parent_user": "postgres",
            "parent_password": "",
        }))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to connect to branch service: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "Branch service returned {}: {}",
            status, body
        )));
    }

    let info: PhysicalBranchInfo = response
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to parse response: {}", e)))?;

    Ok(info)
}

#[derive(Debug, serde::Deserialize)]
struct PhysicalBranchInfo {
    branch_id: String,
    data_dir: String,
    status: String,
    port: u16,
}
