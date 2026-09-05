use anyhow::Result;
use colored::*;
use std::process::Command;

use crate::client::ApiClient;

pub async fn doctor(client: &ApiClient) -> Result<()> {
    println!("{}", "🩺 Freebuff Doctor".cyan().bold());
    println!();
    println!("  Checking your environment for potential issues...");
    println!();

    let mut issues = 0;

    // Check Rust version
    print_check("Rust installed");
    match Command::new("rustc").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            let version = version.trim().replace("rustc ", "");
            pass(&version);
        }
        _ => {
            fail("Rust is not installed");
            println!("    Install: https://rustup.rs");
            issues += 1;
        }
    }

    // Check Cargo
    print_check("Cargo installed");
    match Command::new("cargo").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            let version = version.trim().replace("cargo ", "");
            pass(&version);
        }
        _ => {
            fail("Cargo is not installed");
            issues += 1;
        }
    }

    // Check Node.js
    print_check("Node.js installed");
    match Command::new("node").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            pass(version.trim());
        }
        _ => {
            warn("Node.js not found (optional, needed for dashboard)");
        }
    }

    // Check Docker
    print_check("Docker installed");
    match Command::new("docker").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            let version = version.trim().split(',').next().unwrap_or("unknown");
            pass(version);
        }
        _ => {
            warn("Docker not found (needed for local dev)");
            println!("    Install: https://docs.docker.com/get-docker/");
        }
    }

    // Check Docker Compose
    print_check("Docker Compose installed");
    match Command::new("docker-compose").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            let version = version.trim().split(',').next().unwrap_or("unknown");
            pass(version);
        }
        _ => {
            warn("Docker Compose not found (needed for local dev)");
        }
    }

    // Check Docker daemon
    print_check("Docker daemon running");
    match Command::new("docker").arg("info").output() {
        Ok(output) if output.status.success() => pass("running"),
        _ => {
            warn("Docker daemon not running");
            println!("    Start Docker and try again");
        }
    }

    // Check PostgreSQL client
    print_check("PostgreSQL client (psql)");
    match Command::new("psql").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout);
            pass(version.trim());
        }
        _ => {
            warn("psql not found (optional, useful for direct DB access)");
        }
    }

    // Check API server
    print_check("API server reachable");
    if client.health_check()? {
        pass("connected");
    } else {
        warn("API server not reachable");
        println!("    Start with: {}", "cargo run -p freebuff-control-plane".dimmed());
    }

    println!();

    // Check authentication
    print_check("Authentication");
    let creds = crate::config::Credentials::load()?;
    if creds.is_authenticated() {
        pass(&format!("logged in as {}", creds.email.unwrap_or_else(|| "unknown".into())));
    } else {
        warn("Not authenticated");
        println!("    Run: {}", "freebuff auth login".cyan());
    }

    println!();
    println!("{}", "─".repeat(50).dimmed());

    if issues == 0 {
        println!("{} No critical issues found", "✓".green().bold());
    } else {
        println!("{} {} issue(s) found", "⚠".yellow(), issues.to_string().yellow());
    }

    println!();

    // Show config path
    println!("  {} {}", "Config:".dimmed(), crate::config::Config::config_dir().display());

    Ok(())
}

fn print_check(name: &str) {
    print!("  {} {} ", "●".dimmed(), name);
}

fn pass(detail: &str) {
    println!("{} {}", "✓".green(), detail.dimmed());
}

fn warn(detail: &str) {
    println!("{} {}", "⚠".yellow(), detail.yellow());
}

fn fail(detail: &str) {
    println!("{} {}", "✗".red(), detail.red());
}
