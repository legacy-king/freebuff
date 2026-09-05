mod auth;
mod config;
mod client;
mod commands;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "freebuff",
    about = "🚀 Freebuff CLI — Manage serverless PostgreSQL from the command line",
    version,
    long_about = "Freebuff CLI helps you create, manage, and deploy serverless PostgreSQL databases.\n\nGet started with: freebuff init"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Use a specific config file
    #[arg(long, global = true)]
    config: Option<String>,

    /// Output format (json, table, wide)
    #[arg(long, global = true, default_value = "table")]
    output: String,

    /// Verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// 🔐 Authenticate with Freebuff
    Auth {
        #[command(subcommand)]
        command: AuthCommands,
    },

    /// 🚀 Initialize a new Freebuff project
    Init {
        /// Project name
        #[arg(short, long)]
        name: Option<String>,

        /// Project region
        #[arg(short, long, default_value = "us-east-1")]
        region: String,

        /// Database plan (free, pro, enterprise)
        #[arg(short, long, default_value = "free")]
        plan: String,
    },

    /// 📋 List all projects
    #[command(alias = "ls")]
    Projects {
        #[command(subcommand)]
        command: Option<ProjectCommands>,
    },

    /// 🌿 Manage database branches
    Branch {
        #[command(subcommand)]
        command: BranchCommands,
    },

    /// 📤 Push schema changes to a branch
    Push {
        /// Branch to push to (default: main)
        #[arg(short, long, default_value = "main")]
        branch: String,

        /// SQL file to push
        #[arg(short, long)]
        file: Option<String>,

        /// Dry run — show what would be applied
        #[arg(long)]
        dry_run: bool,

        /// Auto-confirm without prompting
        #[arg(long)]
        yes: bool,
    },

    /// 🔍 Compare schema between branches
    Diff {
        /// Source branch
        #[arg(default_value = "main")]
        from: String,

        /// Target branch
        #[arg(short, long)]
        to: Option<String>,
    },

    /// 📊 Show migration history
    #[command(alias = "history")]
    Migrations {
        #[command(subcommand)]
        command: Option<MigrationCommands>,
    },

    /// 💻 Local development environment
    Dev {
        #[command(subcommand)]
        command: DevCommands,
    },

    /// 🔗 Get connection info for a project
    Connect {
        /// Project name or ID
        project: Option<String>,

        /// Branch to connect to
        #[arg(short, long, default_value = "main")]
        branch: String,

        /// Output as psql command
        #[arg(long)]
        psql: bool,

        /// Output as connection URI
        #[arg(long)]
        uri: bool,
    },

    /// 📈 View project usage and metrics
    Status {
        /// Project name or ID
        project: Option<String>,

        /// Watch mode (refresh every N seconds)
        #[arg(short, long)]
        watch: Option<u64>,
    },

    /// ⚙️  Manage CLI configuration
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },

    /// 🗑️  Delete a project
    Delete {
        /// Project name or ID
        project: String,

        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },

    /// ℹ️  Show version and system info
    Doctor,
}

#[derive(Subcommand)]
pub enum AuthCommands {
    /// Login to Freebuff
    Login {
        /// Email address
        #[arg(short, long)]
        email: Option<String>,
    },

    /// Logout from Freebuff
    Logout,

    /// Show current authentication status
    Whoami,
}

#[derive(Subcommand)]
pub enum ProjectCommands {
    /// List all projects
    List,

    /// Show project details
    Describe {
        /// Project name or ID
        project: String,
    },

    /// Create a new project
    Create {
        /// Project name
        name: String,

        #[arg(short, long, default_value = "us-east-1")]
        region: String,
    },

    /// Delete a project
    Delete {
        /// Project name or ID
        project: String,

        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub enum BranchCommands {
    /// List all branches
    List {
        /// Project name or ID
        #[arg(short, long)]
        project: Option<String>,
    },

    /// Create a new branch
    Create {
        /// Branch name
        name: String,

        /// Parent branch (default: main)
        #[arg(short, long, default_value = "main")]
        parent: String,

        /// Branch at specific LSN
        #[arg(long)]
        lsn: Option<String>,
    },

    /// Delete a branch
    Delete {
        /// Branch name
        name: String,

        #[arg(long)]
        force: bool,
    },

    /// Switch to a branch
    Switch {
        /// Branch name
        name: String,
    },

    /// Show branch diff with parent
    Diff {
        /// Branch name
        name: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum MigrationCommands {
    /// List all migrations
    List,

    /// Create a new migration
    Create {
        /// Migration name
        name: String,
    },

    /// Apply pending migrations
    Run {
        /// Dry run
        #[arg(long)]
        dry_run: bool,
    },

    /// Rollback last migration
    Rollback {
        /// Number of migrations to rollback
        #[arg(default_value = "1")]
        steps: u32,
    },
}

#[derive(Subcommand)]
pub enum DevCommands {
    /// Start local development stack
    Start {
        /// Port for the local Postgres
        #[arg(short, long, default_value = "5432")]
        port: u16,

        /// Skip Docker check
        #[arg(long)]
        skip_docker: bool,
    },

    /// Stop local development stack
    Stop,

    /// Reset local database (drop and recreate)
    Reset {
        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },

    /// Show local development status
    Status,

    /// Run a command in the dev environment
    Exec {
        /// Command to run
        command: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Show current configuration
    Show,

    /// Set a config value
    Set {
        /// Config key
        key: String,

        /// Config value
        value: String,
    },

    /// Get a config value
    Get {
        /// Config key
        key: String,
    },

    /// Reset configuration to defaults
    Reset,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize config
    let config = config::Config::load(cli.config.as_deref())?;

    // Create API client
    let client = client::ApiClient::new(&config)?;

    // Execute command
    match cli.command {
        Commands::Auth { command } => auth::handle_auth_command(command).await?,
        Commands::Init { name, region, plan } => {
            commands::project::init(&client, name, &region, &plan, &cli.output).await?;
        }
        Commands::Projects { command } => {
            commands::project::handle_project_command(command, &client, &cli.output).await?;
        }
        Commands::Branch { command } => {
            commands::branch::handle_branch_command(command, &client, &cli.output).await?;
        }
        Commands::Push { branch, file, dry_run, yes } => {
            commands::push::push(&client, &branch, file.as_deref(), dry_run, yes, &cli.output).await?;
        }
        Commands::Diff { from, to } => {
            commands::diff::diff(&client, &from, to.as_deref(), &cli.output).await?;
        }
        Commands::Migrations { command } => {
            commands::migrations::handle_migration_command(command, &client, &cli.output).await?;
        }
        Commands::Dev { command } => {
            commands::dev::handle_dev_command(command).await?;
        }
        Commands::Connect { project, branch, psql, uri } => {
            commands::connect::connect(&client, project.as_deref(), &branch, psql, uri).await?;
        }
        Commands::Status { project, watch } => {
            commands::status::status(&client, project.as_deref(), watch).await?;
        }
        Commands::Config { command } => {
            commands::config::handle_config_command(command)?;
        }
        Commands::Delete { project, force } => {
            commands::project::delete(&client, &project, force, &cli.output).await?;
        }
        Commands::Doctor => {
            commands::doctor::doctor(&client).await?;
        }
    }

    Ok(())
}
