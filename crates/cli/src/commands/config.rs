use anyhow::Result;
use colored::*;

use crate::ConfigCommands;

pub fn handle_config_command(command: ConfigCommands) -> Result<()> {
    match command {
        ConfigCommands::Show => show_config(),
        ConfigCommands::Set { key, value } => set_config(&key, &value),
        ConfigCommands::Get { key } => get_config(&key),
        ConfigCommands::Reset => reset_config(),
    }
}

fn show_config() -> Result<()> {
    let config = crate::config::Config::load(None)?;

    println!("{}", "⚙️  Configuration".cyan().bold());
    println!();

    println!("  {} {}", "Config file:".dimmed(), crate::config::Config::config_dir().join("config.toml").display());
    println!();

    println!("  {:<20} {}", "api_url:".dimmed(), config.api_url);
    println!("  {:<20} {}", "project_id:".dimmed(), config.project_id.unwrap_or_else(|| "(not set)".into()));
    println!("  {:<20} {}", "default_branch:".dimmed(), config.default_branch);
    println!("  {:<20} {}", "output_format:".dimmed(), config.output_format);
    println!("  {:<20} {}", "region:".dimmed(), config.region);
    println!("  {:<20} {}", "org_id:".dimmed(), config.org_id.unwrap_or_else(|| "(not set)".into()));
    println!();

    // Check credentials
    let creds = crate::config::Credentials::load()?;
    if creds.is_authenticated() {
        println!("  {} Authenticated as {}", "✓".green().bold(), creds.email.unwrap_or_else(|| "unknown".into()));
    } else {
        println!("  {} Not authenticated", "✗".red().bold());
        println!("  Run {} to login", "freebuff auth login".cyan());
    }

    Ok(())
}

fn set_config(key: &str, value: &str) -> Result<()> {
    let mut config = crate::config::Config::load(None)?;
    config.set(key, value)?;

    println!("{} Set {} = {}", "✓".green().bold(), key.cyan(), value);

    Ok(())
}

fn get_config(key: &str) -> Result<()> {
    let config = crate::config::Config::load(None)?;

    match config.get(key) {
        Some(value) => println!("{}", value),
        None => {
            println!("{} Unknown config key: {}", "✗".red(), key);
            println!("  Valid keys: api_url, project_id, default_branch, output_format, region, org_id");
        }
    }

    Ok(())
}

fn reset_config() -> Result<()> {
    let config = crate::config::Config::default();
    config.save()?;

    println!("{} Configuration reset to defaults", "✓".green().bold());

    Ok(())
}
