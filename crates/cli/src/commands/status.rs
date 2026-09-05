use anyhow::Result;
use colored::*;
use std::time::Duration;

use crate::client::ApiClient;

pub async fn status(
    client: &ApiClient,
    project: Option<&str>,
    watch: Option<u64>,
) -> Result<()> {
    loop {
        // Clear screen in watch mode
        if watch.is_some() {
            print!("\x1B[2J\x1B[1;1H");
        }

        println!("{}", "📊 Project Status".cyan().bold());
        println!();

        let project_id = resolve_project(client, project).await?;

        // Get project info
        let project_info: serde_json::Value = client.get(
            &format!("/v1/projects/{}", project_id)
        ).unwrap_or_else(|_| serde_json::json!({
            "data": {
                "id": project_id,
                "name": "unknown",
                "status": "unknown",
                "region": "unknown"
            }
        }));

        let data = &project_info["data"];

        println!("  {} {}", "Project:".dimmed(), data["name"].as_str().unwrap_or("unknown").cyan().bold());
        println!("  {} {}", "ID:".dimmed(), data["id"].as_str().unwrap_or("unknown"));
        println!("  {} {}", "Status:".dimmed(), match data["status"].as_str().unwrap_or("unknown") {
            "active" => "● active".green(),
            "creating" => "○ creating".yellow(),
            "failed" => "✗ failed".red(),
            other => other.normal(),
        });
        println!("  {} {}", "Region:".dimmed(), data["region"].as_str().unwrap_or("unknown"));
        println!();

        // Database info
        if let Some(host) = data["database_host"].as_str() {
            println!("  {}", "Database".cyan().bold());
            println!("  {} {}:{}", "Host:".dimmed(), host, data["database_port"].as_i64().unwrap_or(5432));
            println!("  {} {}", "Database:".dimmed(), data["database_name"].as_str().unwrap_or("postgres"));
            println!();
        }

        // Branches
        match client.get::<Vec<serde_json::Value>>(&format!("/v1/projects/{}/branches", project_id)) {
            Ok(branches) => {
                println!("  {}", "Branches".cyan().bold());
                for branch in &branches {
                    let name = branch["name"].as_str().unwrap_or("unknown");
                    let is_default = branch["is_default"].as_bool().unwrap_or(false);
                    let default_marker = if is_default { " (default)".dimmed() } else { "".normal() };

                    println!("    {} {}{}", "●".green(), name.cyan(), default_marker);
                }
                println!("  {} branches total", branches.len().to_string().cyan());
                println!();
            }
            Err(_) => {}
        }

        // Compute endpoints
        match client.get::<Vec<serde_json::Value>>(&format!("/v1/projects/{}/compute", project_id)) {
            Ok(endpoints) => {
                println!("  {}", "Compute Endpoints".cyan().bold());
                for endpoint in &endpoints {
                    let status = endpoint["status"].as_str().unwrap_or("unknown");
                    let size = endpoint["compute_size"].as_str().unwrap_or("small");
                    let max_conn = endpoint["max_connections"].as_i64().unwrap_or(100);

                    println!("    {} {} ({}) — max {} connections",
                        match status {
                            "running" => "●".green(),
                            "stopped" => "○".dimmed(),
                            _ => "◐".yellow(),
                        },
                        endpoint["id"].as_str().unwrap_or("unknown"),
                        size.cyan(),
                        max_conn
                    );
                }
                println!();
            }
            Err(_) => {}
        }

        // API Keys
        match client.get::<Vec<serde_json::Value>>(&format!("/v1/projects/{}/api-keys", project_id)) {
            Ok(keys) => {
                println!("  {}", "API Keys".cyan().bold());
                for key in &keys {
                    let name = key["name"].as_str().unwrap_or("unknown");
                    let key_type = key["key_type"].as_str().unwrap_or("unknown");
                    let prefix = key["key_prefix"].as_str().unwrap_or("...");

                    println!("    {} {} ({}...)",
                        "●".green(),
                        name.cyan(),
                        prefix.dimmed()
                    );
                }
                println!("  {} keys total", keys.len().to_string().cyan());
                println!();
            }
            Err(_) => {}
        }

        // Timestamp
        println!("  {} {}", "Updated:".dimmed(), chrono::Utc::now().format("%H:%M:%S UTC"));

        // Exit watch mode if not set
        match watch {
            Some(secs) => {
                println!();
                println!("  {} Press Ctrl+C to exit", "Watching...".dimmed());
                tokio::time::sleep(Duration::from_secs(secs)).await;
            }
            None => break,
        }
    }

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
        return Ok(first["id"].as_str().unwrap_or("unknown").to_string());
    }

    anyhow::bail!("No project found. Run {} to create one.", "freebuff init".cyan());
}
