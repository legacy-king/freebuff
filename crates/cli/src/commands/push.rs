use anyhow::{Context, Result};
use colored::*;
use std::path::PathBuf;

use crate::client::ApiClient;

pub async fn push(
    client: &ApiClient,
    branch: &str,
    file: Option<&str>,
    dry_run: bool,
    yes: bool,
    output: &str,
) -> Result<()> {
    println!("{}", "📤 Push Schema Changes".cyan().bold());
    println!();

    // Determine the SQL file to push
    let sql_content = if let Some(path) = file {
        std::fs::read_to_string(path)
            .context(format!("Failed to read SQL file: {}", path))?
    } else {
        // Look for standard migration files
        find_sql_file()?
    };

    if sql_content.trim().is_empty() {
        println!("{} No SQL changes to push", "ℹ".blue());
        return Ok(());
    }

    // Parse SQL statements
    let statements = parse_sql_statements(&sql_content);

    println!("  {} Branch: {}", "Branch:".dimmed(), branch.cyan());
    println!("  {} {} statements found", "Changes:".dimmed(), statements.len().to_string().cyan());
    println!();

    // Show what will be applied
    if dry_run || !yes {
        println!("{}", "📋 Changes to be applied:".yellow());
        println!();
        for (i, stmt) in statements.iter().enumerate() {
            let preview = stmt.trim().chars().take(80).collect::<String>();
            println!("  {}. {}", (i + 1).to_string().cyan(), preview);
            if stmt.trim().len() > 80 {
                println!("     {}", "...".dimmed());
            }
        }
        println!();
    }

    if dry_run {
        println!("{} Dry run — no changes applied", "ℹ".blue());
        return Ok(());
    }

    if !yes {
        let confirm = dialoguer::Confirm::new()
            .with_prompt("Apply these changes?")
            .default(true)
            .interact()?;

        if !confirm {
            println!("{} Push cancelled", "✗".red());
            return Ok(());
        }
    }

    // Apply the changes
    println!();
    println!("{} Applying changes to branch '{}'...", "⚡".cyan().bold(), branch);

    // In production, this would:
    // 1. Send SQL to the branch's Postgres instance
    // 2. Execute statements with proper error handling
    // 3. Record migration in the migration history

    let project_id = resolve_project(client).await?;

    let response: serde_json::Value = client.post(
        &format!("/v1/projects/{}/push", project_id),
        &serde_json::json!({
            "branch": branch,
            "sql": sql_content,
            "statements": statements.len(),
        }),
    )?;

    let migrations_applied = response["migrations_applied"].as_u64().unwrap_or(0);

    println!();
    println!("{} Schema pushed successfully!", "✓".green().bold());
    println!("  {} {} statements applied", "Applied:".dimmed(), migrations_applied.to_string().cyan());
    println!("  {} {}", "Branch:".dimmed(), branch);
    println!();

    Ok(())
}

fn find_sql_file() -> Result<String> {
    // Look for common SQL file locations
    let candidates = vec![
        "migrations/current.sql",
        "migrations/latest.sql",
        "schema.sql",
        "dump.sql",
        "db/schema.sql",
    ];

    for candidate in candidates {
        if std::path::Path::new(candidate).exists() {
            println!("  Found SQL file: {}", candidate.cyan());
            return std::fs::read_to_string(candidate)
                .context(format!("Failed to read {}", candidate));
        }
    }

    // Look for .sql files in current directory
    let entries: Vec<_> = std::fs::read_dir(".")
        .context("Failed to read current directory")?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "sql"))
        .collect();

    if entries.len() == 1 {
        let path = entries[0].path();
        println!("  Found SQL file: {}", path.display().to_string().cyan());
        return std::fs::read_to_string(&path)
            .context(format!("Failed to read {}", path.display()));
    }

    if !entries.is_empty() {
        println!("  Multiple SQL files found:");
        for entry in &entries {
            println!("    - {}", entry.path().display());
        }
        println!();
    }

    // Create a sample migration
    println!("{} No SQL file specified.", "ℹ".blue());
    println!("  Create a migration with: {}", "freebuff migrations create my_migration".cyan());
    println!("  Or specify a file:       {}", "freebuff push --file schema.sql".cyan());

    anyhow::bail!("No SQL file to push");
}

fn parse_sql_statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut in_comment = false;
    let mut in_string = false;

    for line in sql.lines() {
        let trimmed = line.trim();

        // Skip empty lines and full-line comments
        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue;
        }

        // Handle inline comments
        if let Some(comment_pos) = trimmed.find("--") {
            let before_comment = &trimmed[..comment_pos].trim();
            if !before_comment.is_empty() {
                current.push_str(before_comment);
                current.push('\n');
            }
            continue;
        }

        current.push_str(line);
        current.push('\n');

        // Check for statement terminator
        if trimmed.ends_with(';') {
            let stmt = current.trim().to_string();
            if !stmt.is_empty() && stmt != ";" {
                statements.push(stmt);
            }
            current.clear();
        }
    }

    // Don't forget the last statement if it doesn't end with ;
    let last = current.trim().to_string();
    if !last.is_empty() && last != ";" {
        statements.push(last);
    }

    statements
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
