use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::{de::DeserializeOwned, Serialize};

use crate::config::{Config, Credentials};

pub struct ApiClient {
    http: Client,
    base_url: String,
    token: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct ApiError {
    pub error: ApiErrorDetail,
}

#[derive(Debug, serde::Deserialize)]
pub struct ApiErrorDetail {
    pub code: u16,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "API Error ({}): {}", self.error.code, self.error.message)
    }
}

impl ApiClient {
    pub fn new(config: &Config) -> Result<Self> {
        let creds = Credentials::load().unwrap_or_default();

        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            http,
            base_url: config.api_url.clone(),
            token: creds.token.clone(),
        })
    }

    pub fn with_token(mut self, token: String) -> Self {
        self.token = Some(token);
        self
    }

    pub fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);

        let mut request = self.http.get(&url);

        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().context("Failed to send request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();

            // Try to parse as API error
            if let Ok(api_err) = serde_json::from_str::<ApiError>(&body) {
                return Err(anyhow::anyhow!(api_err.to_string()));
            }

            anyhow::bail!("HTTP {} from {}: {}", status, url, body);
        }

        response.json().context("Failed to parse response")
    }

    pub fn post<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: &B) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);

        let mut request = self.http.post(&url).json(body);

        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().context("Failed to send request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();

            if let Ok(api_err) = serde_json::from_str::<ApiError>(&body) {
                return Err(anyhow::anyhow!(api_err.to_string()));
            }

            anyhow::bail!("HTTP {} from {}: {}", status, url, body);
        }

        response.json().context("Failed to parse response")
    }

    pub fn put<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: &B) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);

        let mut request = self.http.put(&url).json(body);

        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().context("Failed to send request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();

            if let Ok(api_err) = serde_json::from_str::<ApiError>(&body) {
                return Err(anyhow::anyhow!(api_err.to_string()));
            }

            anyhow::bail!("HTTP {} from {}: {}", status, url, body);
        }

        response.json().context("Failed to parse response")
    }

    pub fn delete(&self, path: &str) -> Result<()> {
        let url = format!("{}{}", self.base_url, path);

        let mut request = self.http.delete(&url);

        if let Some(ref token) = self.token {
            request = request.bearer_auth(token);
        }

        let response = request.send().context("Failed to send request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();

            if let Ok(api_err) = serde_json::from_str::<ApiError>(&body) {
                return Err(anyhow::anyhow!(api_err.to_string()));
            }

            anyhow::bail!("HTTP {} from {}: {}", status, url, body);
        }

        Ok(())
    }

    /// Check if the server is reachable
    pub fn health_check(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);
        match self.http.get(&url).send() {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}
