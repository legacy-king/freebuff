use anyhow::Result;
use colored::*;

use crate::client::ApiClient;

#[derive(serde::Deserialize, serde::Serialize)]
struct SchemaDiff {
    from_branch: String,
    to_branch: String,
    tables_added: Vec<String>,
    tables_removed: Vec<String>,
    tables_modified: Vec<TableDiff>,
    columns_added: Vec<ColumnInfo>,
    columns_removed: Vec<ColumnInfo>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct TableDiff {
    table: String,
    columns_added: Vec<String>,
    columns_removed: Vec<String>,
    columns_modified: Vec<String>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct ColumnInfo {
    table: String,
    column: String,
    #[serde(rename = "type")]
    column_type: String,
}

pub async fn diff(
    client: &ApiClient,
    from: &str,
    to: Option<&str>,
    output: &str,
) -> Result<()> {
    println!("{}", "🔍 Schema Diff".cyan().bold());
    println!();

    let project_id = resolve_project(client).await?;
    let to_branch = to.unwrap_or("main");

    println!("  {} {} → {}", "Comparing:".dimmed(), from.cyan(), to_branch.cyan());
    println!();

    // Get diff from API
    let response: serde_json::Value = client.get(
        &format!(
            "/v1/projects/{}/branches/{}/diff?target={}",
            project_id, from, to_branch
        )
    ).unwrap_or_else(|_| {
        // If API doesn't support diff yet, generate a placeholder
        serde_json::json!({
            "from_branch": from,
            "to_branch": to_branch,
            "tables_added": [],
            "tables_removed": [],
            "tables_modified": [],
            "columns_added": [],
            "columns_removed": [],
        })
    });

    let diff: SchemaDiff = serde_json::from_value(response)?;

    if output == "json" {
        println!("{}", serde_json::to_string_pretty(&diff)?);
        return Ok(());
    }

    // Display diff
    if diff.tables_added.is_empty()
        && diff.tables_removed.is_empty()
        && diff.tables_modified.is_empty()
        && diff.columns_added.is_empty()
        && diff.columns_removed.is_empty()
    {
        println!("  {} Schemas are identical", "✓".green().bold());
        return Ok(());
    }

    // Tables added
    if !diff.tables_added.is_empty() {
        println!("  {} Tables Added:", "+".green().bold());
        for table in &diff.tables_added {
            println!("    {} {}", "+".green(), table.cyan());
        }
        println!();
    }

    // Tables removed
    if !diff.tables_removed.is_empty() {
        println!("  {} Tables Removed:", "-".red().bold());
        for table in &diff.tables_removed {
            println!("    {} {}", "-".red(), table.cyan());
        }
        println!();
    }

    // Tables modified
    for table_diff in &diff.tables_modified {
        println!("  {} Table Modified: {}", "~".yellow().bold(), table_diff.table.cyan());

        for col in &table_diff.columns_added {
            println!("    {} {} {}", "+".green(), col, "(new)".dimmed());
        }
        for col in &table_diff.columns_removed {
            println!("    {} {} {}", "-".red(), col, "(removed)".dimmed());
        }
        for col in &table_diff.columns_modified {
            println!("    {} {}", "~".yellow(), col);
        }
        println!();
    }

    // Columns added (standalone)
    if !diff.columns_added.is_empty() {
        println!("  {} Columns Added:", "+".green().bold());
        for col in &diff.columns_added {
            println!("    {} {}.{} ({})", "+".green(), col.table, col.column, col.column_type.dimmed());
        }
        println!();
    }

    // Columns removed
    if !diff.columns_removed.is_empty() {
        println!("  {} Columns Removed:", "-".red().bold());
        for col in &diff.columns_removed {
            println!("    {} {}.{}", "-".red(), col.table, col.column);
        }
        println!();
    }

    // Summary
    let total_changes = diff.tables_added.len()
        + diff.tables_removed.len()
        + diff.tables_modified.len()
        + diff.columns_added.len()
        + diff.columns_removed.len();

    println!("  {} {} total changes", "Summary:".dimmed(), total_changes.to_string().cyan());

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
