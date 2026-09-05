use anyhow::Result;
use colored::*;
use dialoguer::{Input, Password};

use crate::commands::AuthCommands;
use crate::config::{Config, Credentials};

pub async fn handle_auth_command(command: AuthCommands) -> Result<()> {
    match command {
        AuthCommands::Login { email } => login(email).await,
        AuthCommands::Logout => logout(),
        AuthCommands::Whoami => whoami(),
    }
}

async fn login(email: Option<String>) -> Result<()> {
    println!("{}", "🔐 Freebuff Login".cyan().bold());
    println!();

    let email = match email {
        Some(e) => e,
        None => Input::new()
            .with_prompt("Email")
            .interact_text()?,
    };

    let password = Password::new()
        .with_prompt("Password")
        .interact()?;

    println!();
    println!("{}", "Connecting to Freebuff...".dimmed());

    let config = Config::load(None)?;
    let client = crate::client::ApiClient::new(&config)?;

    // Call the auth API
    let response: serde_json::Value = client.post("/v1/auth/login", &serde_json::json!({
        "email": email,
        "password": password,
    }))?;

    let token = response["access_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No access token in response"))?
        .to_string();

    let user_name = response["user"]["name"]
        .as_str()
        .map(|s| s.to_string());

    // Save credentials
    let creds = Credentials {
        token: Some(token),
        refresh_token: response["refresh_token"].as_str().map(|s| s.to_string()),
        email: Some(email.clone()),
        name: user_name.clone(),
    };
    creds.save()?;

    println!();
    println!("{} Logged in successfully!", "✓".green().bold());
    if let Some(name) = user_name {
        println!("  Welcome, {}!", name.cyan());
    }
    println!("  Email: {}", email);
    println!();
    println!("  Run {} to get started", "freebuff init".cyan());

    Ok(())
}

fn logout() -> Result<()> {
    Credentials::clear()?;

    println!("{} Logged out successfully", "✓".green().bold());
    println!("  Your credentials have been removed from this machine.");

    Ok(())
}

fn whoami() -> Result<()> {
    let creds = Credentials::load()?;

    if !creds.is_authenticated() {
        println!("{} Not logged in", "✗".red().bold());
        println!("  Run {} to authenticate", "freebuff auth login".cyan());
        return Ok(());
    }

    println!("{}", "👤 Authentication Status".cyan().bold());
    println!();
    println!("  Email: {}", creds.email.unwrap_or_else(|| "unknown".into()));
    if let Some(name) = creds.name {
        println!("  Name:  {}", name);
    }
    let token = creds.token.as_deref().unwrap_or_default();
    println!("  Token: {}...", &token[..token.len().min(20)]);
    println!();

    // Verify token is still valid
    let config = Config::load(None)?;
    let client = crate::client::ApiClient::new(&config)?;

    println!("  {}", "Verifying token...".dimmed());

    match client.get::<serde_json::Value>("/v1/auth/me") {
        Ok(_) => println!("  Status: {}", "✓ Valid".green()),
        Err(_) => println!("  Status: {} (run {} to re-authenticate)", "✗ Expired".red(), "freebuff auth login".cyan()),
    }

    Ok(())
}
