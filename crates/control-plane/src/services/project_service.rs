use uuid::Uuid;

use crate::AppState;
use freebuff_shared::{AppError, CreateProject, Project, ProjectStatus};

/// Create a new project with full infrastructure:
/// 1. Create project record in the database
/// 2. Create the main branch
/// 3. Create a WAL timeline for the project
/// 4. Create a compute endpoint
/// 5. Provision the database instance
pub async fn create_project(state: &AppState, input: CreateProject) -> Result<Project, AppError> {
    // Use first org as default (simplified — real impl would use auth context)
    let org_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001")
        .map_err(|_| AppError::Internal("Invalid default org ID".into()))?;

    let slug = input.slug.clone().unwrap_or_else(|| {
        input
            .name
            .to_lowercase()
            .replace(|c: char| !c.is_alphanumeric() && c != '-', "")
            .replace("--", "-")
            .trim_matches('-')
            .to_string()
    });

    // Step 1: Create project record
    let project = crate::db::projects::create_project(&state.db, org_id, &input, &slug).await?;
    tracing::info!("Created project {} ({})", project.name, project.id);

    // Step 2: Create default main branch
    let branch = crate::db::branches::create_main_branch(&state.db, project.id).await?;
    tracing::info!("Created main branch for project {}", project.id);

    // Step 3: Create WAL timeline for this project
    let timeline_id = project.id.to_string();
    match create_wal_timeline(&timeline_id).await {
        Ok(_) => tracing::info!("Created WAL timeline for project {}", project.id),
        Err(e) => tracing::warn!("Failed to create WAL timeline (non-fatal): {}", e),
    }

    // Step 4: Create a compute endpoint for the main branch
    let _endpoint = crate::db::compute::create_endpoint(&state.db, project.id, branch.id).await?;
    tracing::info!("Created compute endpoint for project {}", project.id);

    // Step 5: Update project status to active and set database info
    let project = crate::db::projects::set_project_database(
        &state.db,
        project.id,
        &format!("db-{}", slug),
        5432,
        &format!("freebuff_{}", slug),
    )
    .await?;

    tracing::info!("Project {} is now active", project.id);

    Ok(project)
}

/// Create a WAL timeline via the WAL service
async fn create_wal_timeline(timeline_id: &str) -> Result<(), AppError> {
    let wal_service_url = std::env::var("WAL_SERVICE_URL")
        .unwrap_or_else(|_| "http://localhost:5001".into());

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/timelines", wal_service_url))
        .json(&serde_json::json!({
            "timeline_id": timeline_id,
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
