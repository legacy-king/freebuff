use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::GatewayState;

pub async fn auth_middleware(
    State(state): State<GatewayState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract API key from Authorization header
    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    let api_key = match auth_header {
        Some(header) => {
            if header.starts_with("Bearer ") {
                &header[7..]
            } else {
                return Err(StatusCode::UNAUTHORIZED);
            }
        }
        None => {
            // Also check apikey header (Supabase-compatible)
            match request.headers().get("apikey").and_then(|v| v.to_str().ok()) {
                Some(key) => key,
                None => return Err(StatusCode::UNAUTHORIZED),
            }
        }
    };

    tracing::debug!("API request with key prefix: {}", &api_key[..api_key.len().min(12)]);

    // In production, this would:
    // 1. Look up the API key hash in the database
    // 2. Validate the key is active and not expired
    // 3. Extract the project_id and scopes
    // 4. Add project context to request extensions

    let response = next.run(request).await;

    // Meter REST API calls for usage-based billing. Counts are batched and
    // flushed to the control plane by the background reporter task.
    let path = response.uri().path().to_string();
    if path.starts_with("/rest/") {
        let mut counts = state.usage_counts.lock().await;
        *counts.entry("api_calls".to_string()).or_insert(0u64) += 1;
        let total = counts.get("api_calls").copied().unwrap_or(0);
        tracing::trace!("Metered API call (total in flight: {})", total);
    }

    Ok(response)
}