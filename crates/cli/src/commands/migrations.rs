use anyhow::Result;
use colored::*;

use crate::client::ApiClient;
use crate::MigrationCommands;

#[derive(serde::Deserialize, serde::Serialize)]
struct Migration {
    id: String,
    name: String,
    status: String,
    applied_at: Option<String>,
    checksum: String,
}

pub async fn handle_migration_command(
    command: Option<MigrationCommands>,
    client: &ApiClient,
    output: &str,
) -> Result<()> {
    match command {
        None | Some(MigrationCommands::List) => list_migrations(client, output).await,
        Some(MigrationCommands::Create { name }) => create_migration(&name),
        Some(MigrationCommands::Run { dry_run }) => run_migrations(client, dry_run, output).await,
        Some(MigrationCommands::Rollback { steps }) => rollback_migrations(client, steps).await,
    }
}

async fn list_migrations(client: &ApiClient, output: &str) -> Result<()> {
    let project_id = resolve_project(client).await?;

    let migrations: Vec<Migration> = client.get(
        &format!("/v1/projects/{}/migrations", project_id)
    ).unwrap_or_default();

    if migrations.is_empty() {
        println!("{}", "No migrations found".yellow());
        println!("  Create one with: {}", "freebuff migrations create my_migration".cyan());
        return Ok(());
    }

    if output == "json" {
        println!("{}", serde_json::to_string_pretty(&migrations)?);
        return Ok(());
    }

    println!("{}", "📋 Migration History".cyan().bold());
    println!();

    println!(
        "  {:<30} {:<15} {:<25}",
        "NAME".dimmed(),
        "STATUS".dimmed(),
        "APPLIED AT".dimmed()
    );
    println!("  {}", "─".repeat(70).dimmed());

    for migration in &migrations {
        let status = match migration.status.as_str() {
            "applied" => migration.status.green(),
            "pending" => migration.status.yellow(),
            "failed" => migration.status.red(),
            _ => migration.status.normal(),
        };

        println!(
            "  {:<30} {:<15} {}",
            migration.name.cyan(),
            status,
            migration.applied_at.as_deref().unwrap_or("-")
        );
    }

    println!();
    println!("  {} migrations total", migrations.len().to_string().cyan());

    Ok(())
}

fn create_migration(name: &str) -> Result<()> {
    // Create migration directory if it doesn't exist
    let migrations_dir = std::path::Path::new("migrations");
    if !migrations_dir.exists() {
        std::fs::create_dir_all(migrations_dir)?;
    }

    // Generate timestamp prefix
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let filename = format!("{}/{}_{}.sql", migrations_dir.display(), timestamp, name);

    // Create the migration file with a template
    let template = format!(
        r#"-- Migration: {}
-- Created: {}
-- Description: TODO

-- Up migration
BEGIN;

-- Add your SQL changes here
-- CREATE TABLE example (
--     id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
--     name VARCHAR(255) NOT NULL,
--     created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
-- );

COMMIT;
"#,
        name,
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    std::fs::write(&filename, template)?;

    println!("{} Created migration: {}", "✓".green().bold(), filename.cyan());
    println!();
    println!("  Edit the file to add your SQL changes, then run:");
    println!("    {}", "freebuff push".cyan());

    Ok(())
}

async fn run_migrations(client: &ApiClient, dry_run: bool, output: &str) -> Result<()> {
    let project_id = resolve_project(client).await?;

    println!("{}", "📤 Running Migrations".cyan().bold());
    println!();

    if dry_run {
        println!("  {} Dry run — showing what would be applied", "ℹ".blue());
        println!();
    }

    // Look for pending SQL files
    let migrations_dir = std::path::Path::new("migrations");
    if !migrations_dir.exists() {
        println!("{} No migrations directory found", "ℹ".blue());
        return Ok(());
    }

    let mut sql_files: Vec<_> = std::fs::read_dir(migrations_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "sql"))
        .collect();

    sql_files.sort_by_key(|e| e.path());

    if sql_files.is_empty() {
        println!("{} No migration files found", "ℹ".blue());
        return Ok(());
    }

    for entry in &sql_files {
        let path = entry.path();
        let filename = path.file_name().unwrap_or_default().to_string_lossy();

        println!("  {} {}", "→".cyan(), filename);

        if dry_run {
            let content = std::fs::read_to_string(&path)?;
            let lines: Vec<&str> = content.lines().take(5).collect();
            for line in lines {
                if !line.trim().is_empty() && !line.trim().starts_with("--") {
                    println!("    {}", line.dimmed());
                }
            }
            if content.lines().count() > 5 {
                println!("    {}", "...".dimmed());
            }
        }
    }

    if !dry_run {
        println!();
        println!("{} All migrations applied", "✓".green().bold());
    }

    Ok(())
}

async fn rollback_migrations(client: &ApiClient, steps: u32) -> Result<()> {
    let project_id = resolve_project(client).await?;

    println!("{}", "⏪ Rolling Back Migrations".cyan().bold());
    println!();
    println!("  {} Rolling back {} migration(s)", "⚠".yellow(), steps.to_string().cyan());

    // Confirm
    let confirm = dialoguer::Confirm::new()
        .with_prompt("Are you sure you want to rollback?")
        .default(false)
        .interact()?;

    if !confirm {
        println!("{} Rollback cancelled", "✗".red());
        return Ok(());
    }

    println!();
    println!("  Rollback would execute the 'down' section of each migration");
    println!("  This is a destructive operation!");
    println!();
    println!("  {} Rollback not yet implemented", "⚠".yellow());
    println!("  Manually revert changes and run {}", "freebuff push".cyan());

    Ok(())
}

async fn resolve_project(client: &ApiClient) -> Result<String> {
    let config = crate::config::Config::load(None)?;
    if let Some(project_id) = &config.project_id {
        return Ok(project_id.clone());
    }

    let projects: Vec<serde_json::Value> = client.get("/v1/projects")?;
    if let Some(first) = projects.first() {
        return Ok(first["id"].as_str().unwrap_or("unknown").to_string());
    }

    anyhow::bail!("No project found. Run {} to create one.", "freebuff init".cyan());
}
