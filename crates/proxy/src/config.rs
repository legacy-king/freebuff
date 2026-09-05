// Proxy-specific configuration that extends the shared config
// Currently using shared::ProxyConfig directly

use freebuff_shared::ProxyConfig;

pub fn validate_config(config: &ProxyConfig) -> Result<(), String> {
    if config.listen_port == 0 {
        return Err("listen_port cannot be 0".into());
    }
    if config.max_connections_per_project == 0 {
        return Err("max_connections_per_project must be > 0".into());
    }
    Ok(())
}
