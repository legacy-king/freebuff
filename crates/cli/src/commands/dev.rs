use anyhow::Result;
use colored::*;
use std::process::Command;

use crate::DevCommands;

pub async fn handle_dev_command(command: DevCommands) -> Result<()> {
    match command {
        DevCommands::Start { port, skip_docker } => start_dev(port, skip_docker).await,
        DevCommands::Stop => stop_dev().await,
        DevCommands::Reset { force } => reset_dev(force).await,
        DevCommands::Status => dev_status().await,
        DevCommands::Exec { command } => exec_dev(command).await,
    }
}

async fn start_dev(port: u16, skip_docker: bool) -> Result<()> {
    println!("{}", "💻 Starting Local Development Environment".cyan().bold());
    println!();

    // Check if Docker is available
    if !skip_docker {
        check_docker()?;
    }

    // Check if port is available
    if is_port_in_use(port) {
        println!("{} Port {} is already in use", "⚠".yellow(), port);
        println!("  Use {} or specify a different port", "freebuff dev stop".cyan());
        anyhow::bail!("Port {} in use", port);
    }

    println!("  {} PostgreSQL on port {}", "→".cyan(), port.to_string().cyan());
    println!("  {} Redis on port {}", "→".cyan(), "6379".cyan());
    println!("  {} MinIO on port {}", "→".cyan(), "9000".cyan());
    println!();

    // Start with Docker Compose
    if !skip_docker {
        println!("{} Starting Docker containers...", "⚡".cyan().bold());

        let status = Command::new("docker-compose")
            .args(["-f", "docker/docker-compose.yml", "up", "-d"])
            .status()?;

        if !status.success() {
            anyhow::bail!("Failed to start Docker containers");
        }
    }

    println!();
    println!("{} Local development environment is ready!", "✓".green().bold());
    println!();
    println!("  Services:");
    println!("    PostgreSQL:  {}", format!("postgresql://freebuff:freebuff@localhost:{}/freebuff", port).cyan());
    println!("    Redis:       {}", "redis://localhost:6379".cyan());
    println!("    MinIO:       {}", "http://localhost:9001 (console)".cyan());
    println!();
    println!("  Useful commands:");
    println!("    {} — Connect to database", "psql postgresql://freebuff:freebuff@localhost:5432/freebuff".dimmed());
    println!("    {} — View logs", "freebuff dev status".dimmed());
    println!("    {} — Stop environment", "freebuff dev stop".dimmed());
    println!();

    Ok(())
}

async fn stop_dev() -> Result<()> {
    println!("{}", "🛑 Stopping Local Development Environment".cyan().bold());
    println!();

    let status = Command::new("docker-compose")
        .args(["-f", "docker/docker-compose.yml", "down"])
        .status()?;

    if status.success() {
        println!("{} Local development environment stopped", "✓".green().bold());
    } else {
        println!("{} Failed to stop some containers", "⚠".yellow());
    }

    Ok(())
}

async fn reset_dev(force: bool) -> Result<()> {
    if !force {
        let confirm = dialoguer::Confirm::new()
            .with_prompt("This will DELETE ALL DATA. Are you sure?")
            .default(false)
            .interact()?;

        if !confirm {
            println!("{} Reset cancelled", "✗".red());
            return Ok(());
        }
    }

    println!("{}", "🔄 Resetting Local Development Environment".cyan().bold());
    println!();

    // Stop containers
    Command::new("docker-compose")
        .args(["-f", "docker/docker-compose.yml", "down", "-v"])
        .status()?;

    // Remove volumes
    println!("  {} Removing volumes...", "→".cyan());
    Command::new("docker")
        .args(["volume", "rm", "freebuff_postgres_data", "freebuff_redis_data", "freebuff_minio_data"])
        .status()
        .ok(); // Ignore errors

    println!();
    println!("{} Environment reset", "✓".green().bold());
    println!("  Run {} to start fresh", "freebuff dev start".cyan());

    Ok(())
}

async fn dev_status() -> Result<()> {
    println!("{}", "📊 Local Development Status".cyan().bold());
    println!();

    // Check Docker containers
    let output = Command::new("docker-compose")
        .args(["-f", "docker/docker-compose.yml", "ps", "--format", "json"])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    if stdout.trim().is_empty() {
        println!("  {} No containers running", "ℹ".blue());
        println!("  Run {} to start the environment", "freebuff dev start".cyan());
        return Ok(());
    }

    println!("  Containers:");
    for line in stdout.lines() {
        if let Ok(container) = serde_json::from_str::<serde_json::Value>(line) {
            let name = container["Name"].as_str().unwrap_or("unknown");
            let state = container["State"].as_str().unwrap_or("unknown");
            let ports = container["Publishers"]
                .as_array()
                .and_then(|p| p.first())
                .and_then(|p| p["PublishedPort"].as_u64())
                .map(|p| format!(":{}", p))
                .unwrap_or_default();

            let status = match state {
                "running" => state.green(),
                "exited" => state.red(),
                _ => state.normal(),
            };

            println!("    {} {} {}", "●".green(), name.cyan(), status);
            if !ports.is_empty() {
                println!("      Port: {}", ports.dimmed());
            }
        }
    }

    println!();

    // Check ports
    println!("  Port Status:");
    let ports = vec![5432, 5433, 6379, 8000, 9000, 9001, 3000, 3001];
    for port in &ports {
        let status = if is_port_in_use(*port) {
            "in use".yellow()
        } else {
            "available".green()
        };
        println!("    {:<8} {}", format!(":{}:", port).dimmed(), status);
    }

    Ok(())
}

async fn exec_dev(command: Vec<String>) -> Result<()> {
    if command.is_empty() {
        anyhow::bail!("No command specified. Usage: freebuff dev exec <command>");
    }

    let status = Command::new("docker-compose")
        .args(["-f", "docker/docker-compose.yml", "exec", "postgres"])
        .args(&command)
        .status()?;

    std::process::exit(status.code().unwrap_or(1));
}

fn check_docker() -> Result<()> {
    let output = Command::new("docker").arg("--version").output()?;

    if !output.status.success() {
        println!("{} Docker is not installed or not in PATH", "✗".red().bold());
        println!("  Install Docker: https://docs.docker.com/get-docker/");
        anyhow::bail!("Docker not found");
    }

    // Check if Docker daemon is running
    let output = Command::new("docker").arg("info").output()?;

    if !output.status.success() {
        println!("{} Docker daemon is not running", "✗".red().bold());
        println!("  Start Docker and try again");
        anyhow::bail!("Docker not running");
    }

    // Check if Docker Compose is available
    let output = Command::new("docker-compose").arg("--version").output()?;

    if !output.status.success() {
        println!("{} Docker Compose is not installed", "✗".red().bold());
        println!("  Install Docker Compose: https://docs.docker.com/compose/install/");
        anyhow::bail!("Docker Compose not found");
    }

    Ok(())
}

fn is_port_in_use(port: u16) -> bool {
    std::net::TcpListener::bind(format!("127.0.0.1:{}", port)).is_err()
}
