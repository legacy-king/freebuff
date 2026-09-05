use anyhow::Result;
use colored::*;

use crate::client::ApiClient;
use crate::ProjectCommands;

#[derive(serde::Deserialize)]
struct Project {
    id: String,
    name: String,
    slug: String,
    region: String,
    status: String,
    database_host: Option<String>,
    database_port: Option<i32>,
    database_name: Option<String>,
    created_at: String,
}

pub async fn init(
    client: &ApiClient,
    name: Option<String>,
    region: &str,
    plan: &str,
    output: &str,
) -> Result<()> {
    println!("{}", "🚀 Initializing Freebuff Project".cyan().bold());
    println!();

    let project_name = match name {
        Some(n) => n,
        None => {
            let input: String = dialoguer::Input::new()
                .with_prompt("Project name")
                .default("my-project".into())
                .interact_text()?;
            input
        }
    };

    println!("  Creating project '{}' in {}", project_name.cyan(), region);
    println!();

    // Create project via API
    let response: serde_json::Value = client.post("/v1/projects", &serde_json::json!({
        "name": project_name,
        "region": region,
        "plan": plan,
    }))?;

    let project = &response["data"];

    println!("{} Project created successfully!", "✓".green().bold());
    println!();
    println!("  {} {}", "Name:".dimmed(), project["name"].as_str().unwrap_or("unknown"));
    println!("  {} {}", "ID:".dimmed(), project["id"].as_str().unwrap_or("unknown"));
    println!("  {} {}", "Region:".dimmed(), region);
    println!();

    // Show connection info
    if let Some(host) = project["database_host"].as_str() {
        let port = project["database_port"].as_i64().unwrap_or(5432);
        let db = project["database_name"].as_str().unwrap_or("postgres");
        let conn_str = format!("postgresql://postgres:[password]@{}:{}/{}", host, port, db);

        println!("  {} {}", "Connection:".dimmed(), conn_str.cyan());
        println!();
    }

    println!("  Next steps:");
    println!("    1. {} — View your project", "freebuff projects describe".cyan());
    println!("    2. {} — Create a branch for development", "freebuff branch create dev".cyan());
    println!("    3. {} — Push schema changes", "freebuff push".cyan());
    println!();

    Ok(())
}

pub async fn handle_project_command(
    command: Option<ProjectCommands>,
    client: &ApiClient,
    output: &str,
) -> Result<()> {
    match command {
        None | Some(ProjectCommands::List) => list_projects(client, output).await,
        Some(ProjectCommands::Describe { project }) => describe_project(client, &project, output).await,
        Some(ProjectCommands::Create { name, region }) => {
            init(client, Some(name), &region, "free", output).await
        }
        Some(ProjectCommands::Delete { project, force }) => delete(client, &project, force, output).await,
    }
}

async fn list_projects(client: &ApiClient, output: &str) -> Result<()> {
    let projects: Vec<Project> = client.get("/v1/projects")?;

    if projects.is_empty() {
        println!("{}", "No projects found".yellow());
        println!("  Run {} to create your first project", "freebuff init".cyan());
        return Ok(());
    }

    if output == "json" {
        println!("{}", serde_json::to_string_pretty(&projects)?);
        return Ok(());
    }

    println!("{}", "📋 Projects".cyan().bold);
    println!();

    // Table header
    println!(
        "  {:<30} {:<20} {:<15} {:<15}",
        "NAME".dimmed(),
        "ID".dimmed(),
        "REGION".dimmed(),
        "STATUS".dimmed()
    );
    println!("  {}", "─".repeat(80).dimmed());

    for project in &projects {
        let status_color = match project.status.as_str() {
            "active" => project.status.green(),
            "creating" => project.status.yellow(),
            "failed" => project.status.red(),
            _ => project.status.normal(),
        };

        println!(
            "  {:<30} {:<20} {:<15} {}",
            project.name.cyan(),
            &project.id[..8],
            project.region,
            status_color
        );
    }

    println!();
    println!("  {} projects total", projects.len().to_string().cyan());

    Ok(())
}

async fn describe_project(client: &ApiClient, project_id: &str, output: &str) -> Result<()> {
    let response: serde_json::Value = client.get(&format!("/v1/projects/{}", project_id))?;
    let project: Project = serde_json::from_value(response["data"].clone())?;

    if output == "json" {
        println!("{}", serde_json::to_string_pretty(&project)?);
        return Ok(());
    }

    println!("{}", "📊 Project Details".cyan().bold());
    println!();
    println!("  {} {}", "Name:".dimmed(), project.name.cyan().bold());
    println!("  {} {}", "ID:".dimmed(), project.id);
    println!("  {} {}", "Slug:".dimmed(), project.slug);
    println!("  {} {}", "Region:".dimmed(), project.region);
    println!("  {} {}", "Status:".dimmed(), match project.status.as_str() {
        "active" => project.status.green(),
        _ => project.status.normal(),
    });
    println!();

    if let Some(host) = &project.database_host {
        println!("  {}", "Database Connection".cyan().bold());
        println!("  {} {}", "Host:".dimmed(), host);
        println!("  {} {}", "Port:".dimmed(), project.database_port.map_or("5432".into(), |p| p.to_string()));
        println!("  {} {}", "Database:".dimmed(), project.database_name.as_deref().unwrap_or("postgres"));
        println!();
    }

    println!("  {} {}", "Created:".dimmed(), project.created_at);

    Ok(())
}

pub async fn delete(client: &ApiClient, project_id: &str, force: bool, _output: &str) -> Result<()> {
    if !force {
        let confirm: String = dialoguer::Input::new()
            .with_prompt(format!(
                "Are you sure you want to delete project {}? Type the project ID to confirm",
                project_id
            ))
            .interact_text()?;

        if confirm != project_id {
            println!("{} Deletion cancelled", "✗".red());
            return Ok(());
        }
    }

    println!("Deleting project {}...", project_id);

    client.delete(&format!("/v1/projects/{}", project_id))?;

    println!("{} Project deleted", "✓".green().bold());

    Ok(())
}
