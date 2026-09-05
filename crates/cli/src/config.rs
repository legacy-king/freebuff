use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const CONFIG_FILE: &str = "config.toml";
const CREDENTIALS_FILE: &str = "credentials.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Freebuff API endpoint
    pub api_url: String,

    /// Default project ID
    pub project_id: Option<String>,

    /// Default branch
    pub default_branch: String,

    /// Output format
    pub output_format: String,

    /// Current region
    pub region: String,

    /// Current organization ID
    pub org_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    /// Auth token
    pub token: Option<String>,

    /// Refresh token
    pub refresh_token: Option<String>,

    /// User email
    pub email: Option<String>,

    /// User name
    pub name: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_url: "http://localhost:3001".into(),
            project_id: None,
            default_branch: "main".into(),
            output_format: "table".into(),
            region: "us-east-1".into(),
            org_id: None,
        }
    }
}

impl Config {
    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("freebuff")
    }

    pub fn credentials_path() -> PathBuf {
        Self::config_dir().join(CREDENTIALS_FILE)
    }

    pub fn load(custom_path: Option<&str>) -> Result<Self> {
        let path = if let Some(p) = custom_path {
            PathBuf::from(p)
        } else {
            Self::config_dir().join(CONFIG_FILE)
        };

        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .context("Failed to read config file")?;
            let config: Config = toml::from_str(&content)
                .context("Failed to parse config file")?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::config_dir();
        std::fs::create_dir_all(&dir)?;

        let path = dir.join(CONFIG_FILE);
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "api_url" => self.api_url = value.to_string(),
            "project_id" => self.project_id = Some(value.to_string()),
            "default_branch" => self.default_branch = value.to_string(),
            "output_format" => self.output_format = value.to_string(),
            "region" => self.region = value.to_string(),
            "org_id" => self.org_id = Some(value.to_string()),
            _ => anyhow::bail!("Unknown config key: {}", key),
        }
        self.save()
    }

    pub fn get(&self, key: &str) -> Option<String> {
        match key {
            "api_url" => Some(self.api_url.clone()),
            "project_id" => self.project_id.clone(),
            "default_branch" => Some(self.default_branch.clone()),
            "output_format" => Some(self.output_format.clone()),
            "region" => Some(self.region.clone()),
            "org_id" => self.org_id.clone(),
            _ => None,
        }
    }
}

impl Credentials {
    pub fn load() -> Result<Self> {
        let path = Config::credentials_path();

        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .context("Failed to read credentials file")?;
            let creds: Credentials = toml::from_str(&content)
                .context("Failed to parse credentials file")?;
            Ok(creds)
        } else {
            Ok(Credentials {
                token: None,
                refresh_token: None,
                email: None,
                name: None,
            })
        }
    }

    pub fn save(&self) -> Result<()> {
        let dir = Config::config_dir();
        std::fs::create_dir_all(&dir)?;

        let path = dir.join(CREDENTIALS_FILE);
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;

        // Set restrictive permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    pub fn clear() -> Result<()> {
        let path = Config::credentials_path();
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    pub fn is_authenticated(&self) -> bool {
        self.token.is_some()
    }

    pub fn token(&self) -> Option<&str> {
        self.token.as_deref()
    }
}
