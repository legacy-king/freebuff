use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use freebuff_shared::{AppError, ProxyConfig};

#[derive(Debug, Clone)]
pub struct ProjectPool {
    pub host: String,
    pub port: i32,
    pub database: String,
    pub max_connections: u32,
    pub active_connections: u32,
}

pub struct PoolManager {
    pools: RwLock<HashMap<String, Arc<ProjectPool>>>,
    config: ProxyConfig,
}

impl PoolManager {
    pub async fn new(config: ProxyConfig) -> Result<Self, AppError> {
        Ok(Self {
            pools: RwLock::new(HashMap::new()),
            config,
        })
    }

    pub async fn get_or_create_pool(&self, project_id: &str) -> Result<Arc<ProjectPool>, AppError> {
        // Check if pool already exists
        {
            let pools = self.pools.read().await;
            if let Some(pool) = pools.get(project_id) {
                return Ok(pool.clone());
            }
        }

        // Create new pool — in production, this would fetch connection info
        // from the control plane and set up a real connection pool
        let pool = Arc::new(ProjectPool {
            host: format!("db-{}", project_id),
            port: 5432,
            database: format!("freebuff_{}", project_id),
            max_connections: self.config.max_connections_per_project,
            active_connections: 0,
        });

        let mut pools = self.pools.write().await;
        pools.insert(project_id.to_string(), pool.clone());

        tracing::info!(
            "Created connection pool for project {} -> {}:{}",
            project_id,
            pool.host,
            pool.port
        );

        Ok(pool)
    }

    pub async fn remove_pool(&self, project_id: &str) {
        let mut pools = self.pools.write().await;
        pools.remove(project_id);
        tracing::info!("Removed connection pool for project {}", project_id);
    }

    pub async fn pool_count(&self) -> usize {
        self.pools.read().await.len()
    }
}
