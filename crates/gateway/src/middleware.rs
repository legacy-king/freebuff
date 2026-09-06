use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};

use crate::{GatewayState, routes::rest::ForwardedToken};

pub async fn auth_middleware(
    State(state): State<GatewayState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Accept either a Bearer JWT or a raw apikey header (Supabase-compatible).
    // The gateway does not validate JWTs itself — it forwards Bearer tokens to the
    // control plane, which owns the JWT secret. Raw apikey headers are accepted but
    // not yet resolved to a project; that is the next auth step.
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let forwarded_token: Option<String> = match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            Some(header[7..].to_string())
        }
        _ => {
            // apikey header accepted but not forwarded (no validation done here)
            request.headers().get("apikey").and_then(|v| v.to_str().ok()).map(|k| k.to_string())
        }
    };

    if let Some(token) = forwarded_token {
        request.extensions_mut().insert(ForwardedToken(token));
        tracing::debug!("Forwarded Bearer token to control plane (prefix: {:?})", &forwarded_token.as_deref()[..forwarded_token.as_deref().len().min(12)]);
    } else {
        tracing::debug!("No Bearer token to forward");
    }

    let path = request.uri().path().to_string();
    let response = next.run(request).await;

    // Meter REST API calls for usage-based billing. Counts are batched and
    // flushed to the control plane by the background reporter task.
    if path.starts_with("/rest/") {
        let mut counts = state.usage_counts.lock().await;
        *counts.entry("api_calls".to_string()).or_insert(0u64) += 1;
        let _total = counts.get("api_calls").copied().unwrap_or(0);
        tracing::trace!("Metered API call (total in flight: {})", _total);
    }

    Ok(response)
}
