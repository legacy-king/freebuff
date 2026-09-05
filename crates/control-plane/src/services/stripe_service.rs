use axum::http::Method;
use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::Sha256;

use crate::AppState;
use freebuff_shared::{AppError, Config};

type HmacSha256 = Hmac<Sha256>;

const STRIPE_API_BASE: &str = "https://api.stripe.com/v1";

/// Send an authenticated request to the Stripe API and return the JSON body.
async fn stripe_request<T: Serialize>(
    config: &Config,
    http: &reqwest::Client,
    method: Method,
    path: &str,
    body: Option<&T>,
) -> Result<serde_json::Value, AppError> {
    let url = format!("{}{}", STRIPE_API_BASE, path);
    let mut request = http.request(method, &url).bearer_auth(&config.stripe_secret_key);
    if let Some(body) = body {
        request = request.json(body);
    }

    let response = request
        .send()
        .await
        .map_err(|e| AppError::Unavailable(format!("Stripe request failed: {}", e)))?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);

    if !status.is_success() {
        let message = json
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or(text.as_str());
        return Err(AppError::Internal(format!(
            "Stripe API {} {}: {}",
            status.as_u16(),
            path,
            message
        )));
    }

    Ok(json)
}

/// Create a Stripe customer for an organization.
pub async fn create_customer(
    config: &Config,
    http: &reqwest::Client,
    email: &str,
    name: &str,
) -> Result<String, AppError> {
    let body = serde_json::json!({ "email": email, "name": name });
    let json = stripe_request(config, http, Method::POST, "/customers", Some(&body)).await?;
    json.get("id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| AppError::Internal("Stripe customer response missing id".into()))
}

/// Create a Checkout Session for a subscription and return its hosted URL.
/// Requires a base plan price (STRIPE_PRICE_ID); metered usage is billed via
/// Billing Meters attached to that price in the Stripe dashboard.
pub async fn create_checkout_session(
    config: &Config,
    http: &reqwest::Client,
    customer_id: &str,
    price_id: &str,
    success_url: &str,
    cancel_url: &str,
) -> Result<String, AppError> {
    let body = serde_json::json!({
        "mode": "subscription",
        "customer": customer_id,
        "line_items": [{ "price": price_id, "quantity": 1 }],
        "success_url": success_url,
        "cancel_url": cancel_url,
    });
    let json = stripe_request(config, http, Method::POST, "/checkout/sessions", Some(&body)).await?;
    json.get("url")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| AppError::Internal("Stripe checkout response missing url".into()))
}

/// Create a Billing Portal session and return its hosted URL.
pub async fn create_portal_session(
    config: &Config,
    http: &reqwest::Client,
    customer_id: &str,
    return_url: &str,
) -> Result<String, AppError> {
    let body = serde_json::json!({ "customer": customer_id, "return_url": return_url });
    let json = stripe_request(config, http, Method::POST, "/billing_portal/sessions", Some(&body)).await?;
    json.get("url")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| AppError::Internal("Stripe portal response missing url".into()))
}

/// Cancel a subscription at the end of the current billing period.
pub async fn cancel_subscription(
    config: &Config,
    http: &reqwest::Client,
    subscription_id: &str,
) -> Result<(), AppError> {
    let body = serde_json::json!({ "cancel_at_period_end": true });
    stripe_request(
        config,
        http,
        Method::POST,
        &format!("/subscriptions/{}", subscription_id),
        Some(&body),
    )
    .await?;
    Ok(())
}

/// Submit a single usage event to a Stripe Billing Meter.
///
/// The `identifier` is used by Stripe to deduplicate identical events, so
/// retries after a lost response are safe.
pub async fn submit_meter_event(
    config: &Config,
    http: &reqwest::Client,
    event_name: &str,
    identifier: &str,
    customer_id: &str,
    value: f64,
) -> Result<(), AppError> {
    let body = serde_json::json!({
        "event_name": event_name,
        "identifier": identifier,
        "payload": {
            "value": value,
            "stripe_customer_id": customer_id,
        },
    });
    stripe_request(config, http, Method::POST, "/billing/meter_events", Some(&body)).await?;
    Ok(())
}

/// Verify a Stripe webhook signature (`Stripe-Signature` header).
pub fn verify_webhook_signature(
    payload: &[u8],
    signature_header: &str,
    secret: &str,
) -> Result<(), AppError> {
    let mut timestamp: Option<&str> = None;
    let mut signature: Option<&str> = None;
    for part in signature_header.split(',') {
        if let Some(value) = part.strip_prefix("t=") {
            timestamp = Some(value);
        } else if let Some(value) = part.strip_prefix("v1=") {
            signature = Some(value);
        }
    }

    let timestamp = timestamp.ok_or_else(|| AppError::Unauthorized("Webhook missing timestamp".into()))?;
    let signature = signature.ok_or_else(|| AppError::Unauthorized("Webhook missing signature".into()))?;

    let parsed: i64 = timestamp
        .parse()
        .map_err(|_| AppError::Unauthorized("Webhook timestamp invalid".into()))?;
    let skew = (Utc::now().timestamp() - parsed).abs();
    if skew > 300 {
        return Err(AppError::Unauthorized("Webhook timestamp too old".into()));
    }

    let signed_payload = format!("{}.{}", timestamp, String::from_utf8_lossy(payload));
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| AppError::Internal(format!("HMAC init failed: {}", e)))?;
    mac.update(signed_payload.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());

    if expected == signature {
        Ok(())
    } else {
        Err(AppError::Unauthorized("Webhook signature mismatch".into()))
    }
}

/// Apply a verified Stripe webhook event to local billing state.
pub async fn handle_webhook(state: &AppState, event: &serde_json::Value) -> Result<(), AppError> {
    let event_type = event
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let object = event.get("data").and_then(|d| d.get("object"));

    match event_type {
        "checkout.session.completed" => {
            let customer = object
                .and_then(|o| o.get("customer"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let subscription = object
                .and_then(|o| o.get("subscription"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let org_id = object
                .and_then(|o| o.get("client_reference_id"))
                .and_then(|v| v.as_str())
                .and_then(|v| uuid::Uuid::parse_str(v).ok());

            if let Some(org_id) = org_id {
                let account = crate::db::billing::get_billing_account(&state.db, org_id).await?;
                if let Some(account) = account {
                    let account = crate::db::billing::set_stripe_customer(
                        &state.db,
                        org_id,
                        customer,
                        account.billing_email.as_deref(),
                    )
                    .await?;
                    if !subscription.is_empty() {
                        crate::db::billing::update_subscription(
                            &state.db,
                            org_id,
                            subscription,
                            &account.plan,
                            "active",
                        )
                        .await?;
                    }
                    tracing::info!("Checkout completed for org {}", org_id);
                }
            }
        }
        "customer.subscription.created" | "customer.subscription.updated" => {
            let subscription = object
                .and_then(|o| o.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let customer = object
                .and_then(|o| o.get("customer"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let status = object
                .and_then(|o| o.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let plan = object
                .and_then(|o| o.get("items"))
                .and_then(|i| i.get("data"))
                .and_then(|d| d.get(0))
                .and_then(|item| item.get("price"))
                .and_then(|p| p.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("pro");

            let org_id = crate::db::billing::org_id_for_customer(&state.db, customer).await?;
            if let Some(org_id) = org_id {
                crate::db::billing::update_subscription(
                    &state.db,
                    org_id,
                    subscription,
                    plan,
                    status,
                )
                .await?;
                tracing::info!("Subscription {} for org {} is now {}", subscription, org_id, status);
            }
        }
        "customer.subscription.deleted" => {
            let customer = object
                .and_then(|o| o.get("customer"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let org_id = crate::db::billing::org_id_for_customer(&state.db, customer).await?;
            if let Some(org_id) = org_id {
                crate::db::billing::set_billing_status(&state.db, org_id, "canceled").await?;
            }
        }
        "invoice.paid" => {
            let customer = object
                .and_then(|o| o.get("customer"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let org_id = crate::db::billing::org_id_for_customer(&state.db, customer).await?;
            if let Some(org_id) = org_id {
                crate::db::billing::set_billing_status(&state.db, org_id, "active").await?;
            }
        }
        "invoice.payment_failed" => {
            let customer = object
                .and_then(|o| o.get("customer"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let org_id = crate::db::billing::org_id_for_customer(&state.db, customer).await?;
            if let Some(org_id) = org_id {
                crate::db::billing::set_billing_status(&state.db, org_id, "past_due").await?;
                tracing::warn!("Payment failed for org {}", org_id);
            }
        }
        _ => {
            tracing::debug!("Ignoring Stripe webhook event type {}", event_type);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webhook_signature_roundtrip() {
        let secret = "whsec_test_secret";
        let payload = br#"{"id":"evt_1","type":"invoice.paid"}"#;
        let timestamp = Utc::now().timestamp().to_string();
        let signed = format!("{}.{}", timestamp, String::from_utf8_lossy(payload));
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(signed.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());
        let header = format!("t={},v1={}", timestamp, signature);

        assert!(verify_webhook_signature(payload, &header, secret).is_ok());
        assert!(verify_webhook_signature(payload, &header, "whsec_wrong").is_err());
    }

    #[test]
    fn webhook_signature_rejects_stale_timestamp() {
        let secret = "whsec_test_secret";
        let payload = br#"{"id":"evt_1"}"#;
        let old = (Utc::now().timestamp() - 600).to_string();
        let header = format!("t={},v1=deadbeef", old);
        assert!(verify_webhook_signature(payload, &header, secret).is_err());
    }
}