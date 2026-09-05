use anyhow::Result;
use colored::*;

use crate::client::ApiClient;

pub async fn connect(
    client: &ApiClient,
    project: Option<&str>,
    branch: &str,
    psql: bool,
    uri: bool,
) -> Result<()> {
    let project_id = resolve_project(client, project).await?;

    // Get connection info from the API
    let response: serde_json::Value = client.get(
        &format!("/v1/projects/{}/connection", project_id)
    )?;

    let host = response["host"].as_str().unwrap_or("localhost");
    let port = response["port"].as_i64().unwrap_or(5432);
    let database = response["database"].as_str().unwrap_or("postgres");
    let role = response["role"].as_str().unwrap_or("postgres");

    let connection_uri = format!(
        "postgresql://{}:[password]@{}:{}/{}?sslmode=require",
        role, host, port, database
    );

    if psql {
        // Output psql command
        println!("psql \"{}\"", connection_uri);
        return Ok(());
    }

    if uri {
        // Output just the URI
        println!("{}", connection_uri);
        return Ok(());
    }

    // Full connection info display
    println!("{}", "🔗 Connection Info".cyan().bold());
    println!();
    println!("  {} {}", "Project:".dimmed(), project_id.cyan());
    println!("  {} {}", "Branch:".dimmed(), branch.cyan());
    println!();
    println!("  {}", "PostgreSQL URI".cyan().bold());
    println!("  {}", connection_uri.cyan());
    println!();

    // Copy to clipboard hint
    println!("  {} {}", "💡".yellow(), "Copy this URI to connect with any PostgreSQL client".dimmed());
    println!();

    // Example commands
    println!("  {}", "Quick Connect".cyan().bold());
    println!("    psql \"{}\"", connection_uri.dimmed());
    println!();

    // With psql flags
    println!("  {}", "With Options".cyan().bold());
    println!("    psql -h {} -p {} -U {} -d {}", host, port, role, database);
    println!();

    // Connection pooler (PgBouncer-compatible)
    let pooler_uri = format!(
        "postgresql://{}:[password]@{}:{}/{}?sslmode=require&pgbouncer=true",
        role, host, port, database
    );
    println!("  {}", "Connection Pooler".cyan().bold());
    println!("  {}", pooler_uri.dimmed());
    println!("  (Use this for serverless environments with many short-lived connections)");

    Ok(())
}

async fn resolve_project(client: &ApiClient, project: Option<&str>) -> Result<String> {
    if let Some(p) = project {
        return Ok(p.to_string());
    }

    let config = crate::config::Config::load(None)?;
    if let Some(project_id) = &config.project_id {
        return Ok(project_id.clone());
    }

    let projects: Vec<serde_json::Value> = client.get("/v1/projects")?;
    if let Some(first) = projects.first() {
        let name = first["name"].as_str().unwrap_or("unknown");
        let id = first["id"].as_str().unwrap_or("unknown").to_string();
        println!("  Using project: {}", name.cyan());
        return Ok(id);
    }

    anyhow::bail!("No project found. Run {} to create one.", "freebuff init".cyan());
}
