use anyhow::Result;
use colored::*;

use crate::client::ApiClient;
use crate::BranchCommands;

#[derive(serde::Deserialize)]
struct Branch {
    id: String,
    name: String,
    slug: String,
    status: String,
    is_default: bool,
    parent_branch_id: Option<String>,
    parent_lsn: Option<String>,
    created_at: String,
}

pub async fn handle_branch_command(
    command: BranchCommands,
    client: &ApiClient,
    output: &str,
) -> Result<()> {
    match command {
        BranchCommands::List { project } => list_branches(client, project.as_deref(), output).await,
        BranchCommands::Create { name, parent, lsn } => {
            create_branch(client, &name, &parent, lsn.as_deref(), output).await
        }
        BranchCommands::Delete { name, force } => delete_branch(client, &name, force).await,
        BranchCommands::Switch { name } => switch_branch(&name),
        BranchCommands::Diff { name } => show_branch_diff(client, name.as_deref()).await,
    }
}

async fn list_branches(client: &ApiClient, project: Option<&str>, output: &str) -> Result<()> {
    let project_id = resolve_project(client, project).await?;

    let branches: Vec<Branch> = client.get(&format!("/v1/projects/{}/branches", project_id))?;

    if branches.is_empty() {
        println!("{}", "No branches found".yellow());
        return Ok(());
    }

    if output == "json" {
        println!("{}", serde_json::to_string_pretty(&branches)?);
        return Ok(());
    }

    println!("{}", "🌿 Branches".cyan().bold());
    println!();

    println!(
        "  {:<25} {:<15} {:<15} {:<10}",
        "NAME".dimmed(),
        "STATUS".dimmed(),
        "LSN".dimmed(),
        "DEFAULT".dimmed()
    );
    println!("  {}", "─".repeat(65).dimmed());

    for branch in &branches {
        let default_marker = if branch.is_default {
            "✓".green()
        } else {
            "".normal()
        };

        let lsn = branch.parent_lsn.as_deref().unwrap_or("-");

        println!(
            "  {:<25} {:<15} {:<15} {}",
            branch.name.cyan(),
            match branch.status.as_str() {
                "active" => branch.status.green(),
                _ => branch.status.normal(),
            },
            lsn,
            default_marker
        );
    }

    println!();
    println!("  {} branches total", branches.len().to_string().cyan());

    Ok(())
}

async fn create_branch(
    client: &ApiClient,
    name: &str,
    parent: &str,
    lsn: Option<&str>,
    output: &str,
) -> Result<()> {
    let config = crate::config::Config::load(None)?;
    let project_id = resolve_project(client, None).await?;

    println!("{} Branch '{}' from '{}'", "🌿 Creating".cyan().bold(), name.cyan(), parent);

    let mut body = serde_json::json!({
        "name": name,
        "parent_branch_id": parent,
    });

    if let Some(lsn) = lsn {
        body["parent_lsn"] = serde_json::json!(lsn);
        println!("  at LSN: {}", lsn);
    }

    let response: serde_json::Value = client.post(
        &format!("/v1/projects/{}/branches", project_id),
        &body,
    )?;

    let branch = &response["data"];

    println!();
    println!("{} Branch created!", "✓".green().bold());
    println!("  {} {}", "Name:".dimmed(), name.cyan());
    println!("  {} {}", "ID:".dimmed(), branch["id"].as_str().unwrap_or("unknown"));
    println!("  {} {}", "Status:".dimmed(), "active".green());
    println!();

    if output == "json" {
        println!("{}", serde_json::to_string_pretty(branch)?);
    }

    Ok(())
}

async fn delete_branch(client: &ApiClient, name: &str, force: bool) -> Result<()> {
    let project_id = resolve_project(client, None).await?;

    if !force {
        let confirm: bool = dialoguer::Confirm::new()
            .with_prompt(format!("Delete branch '{}'?", name))
            .default(false)
            .interact()?;

        if !confirm {
            println!("{} Deletion cancelled", "✗".red());
            return Ok(());
        }
    }

    // Get branch ID
    let branches: Vec<Branch> = client.get(&format!("/v1/projects/{}/branches", project_id))?;
    let branch = branches.iter().find(|b| b.name == name || b.slug == name)
        .ok_or_else(|| anyhow::anyhow!("Branch '{}' not found", name))?;

    client.delete(&format!("/v1/projects/{}/branches/{}", project_id, branch.id))?;

    println!("{} Branch '{}' deleted", "✓".green().bold(), name);

    Ok(())
}

fn switch_branch(name: &str) -> Result<()> {
    let mut config = crate::config::Config::load(None)?;
    config.default_branch = name.to_string();
    config.save()?;

    println!("{} Switched to branch '{}'", "✓".green().bold(), name.cyan());
    println!("  This branch will be used by default for push, diff, and other commands.");

    Ok(())
}

async fn show_branch_diff(client: &ApiClient, name: Option<&str>) -> Result<()> {
    let project_id = resolve_project(client, None).await?;
    let config = crate::config::Config::load(None)?;
    let branch_name = name.unwrap_or(&config.default_branch);

    let branches: Vec<Branch> = client.get(&format!("/v1/projects/{}/branches", project_id))?;
    let branch = branches.iter().find(|b| b.name == branch_name || b.slug == branch_name)
        .ok_or_else(|| anyhow::anyhow!("Branch '{}' not found", branch_name))?;

    let parent_id = branch.parent_branch_id.as_deref().unwrap_or("main");
    let parent = branches.iter().find(|b| b.id == parent_id || b.name == parent_id);

    println!("{}", format!("🔍 Diff: {} → {}", parent.map(|p| p.name.as_str()).unwrap_or("unknown"), branch_name).cyan().bold());
    println!();

    if let Some(parent) = parent {
        println!("  {} Branches share history up to LSN {}", "ℹ".blue(), branch.parent_lsn.as_deref().unwrap_or("0/0"));
        println!("  {} Parent: {} (created {})", "ℹ".blue(), parent.name, &parent.created_at[..10]);
    }

    println!();
    println!("  {} Schema diff would show table, index, and function changes", "💡".yellow());
    println!("  Run {} to compare schemas in detail", "freebuff db diff".cyan());

    Ok(())
}

/// Resolve a project ID from a name or the current default
async fn resolve_project(client: &ApiClient, project: Option<&str>) -> Result<String> {
    if let Some(p) = project {
        // Try to find by name first
        let projects: Vec<serde_json::Value> = client.get("/v1/projects")?;
        if let Some(found) = projects.iter().find(|proj| {
            proj["name"].as_str() == Some(p) || proj["slug"].as_str() == Some(p) || proj["id"].as_str() == Some(p)
        }) {
            return Ok(found["id"].as_str().unwrap_or(p).to_string());
        }
        return Ok(p.to_string());
    }

    // Use default project from config
    let config = crate::config::Config::load(None)?;
    if let Some(project_id) = &config.project_id {
        return Ok(project_id.clone());
    }

    // List projects and use first one
    let projects: Vec<serde_json::Value> = client.get("/v1/projects")?;
    if let Some(first) = projects.first() {
        let id = first["id"].as_str().unwrap_or("unknown").to_string();
        println!("  Using project: {}", first["name"].as_str().unwrap_or("unknown").cyan());
        return Ok(id);
    }

    anyhow::bail!("No project found. Run {} to create one.", "freebuff init".cyan());
}
