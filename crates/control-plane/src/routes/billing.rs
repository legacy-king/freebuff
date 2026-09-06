use axum::{
    body::Bytes,
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use axum_extra::headers::authorization::{Authorization, Bearer};
use axum_extra::TypedHeader;
use chrono::{DateTime, Datelike, Utc};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;
use freebuff_shared::{
    api::{ApiResponse, MessageResponse},
    AppError, BillingAccountPublic, DailyUsage, MeterUsage, StripeSessionRequest,
    StripeSessionResponse, UsageSummary,
};

const DEFAULT_ORG_ID: &str = "00000000-0000-0000-0000-000000000001";

/// Resolve the acting org. Prefers the org claim in the JWT when present;
/// otherwise falls back to the seeded default org (consistent with the
/// control plane's current placeholder auth).
fn resolve_org(state: &AppState, bearer: Option<&str>) -> Uuid {
    if let Some(token) = bearer {
        if let Ok(claims) =
            freebuff_shared::auth::decode_access_token(token, &state.config.jwt_secret)
        {
            if let Some(org) = claims.org_id() {
                return org;
            }
        }
    }
    Uuid::parse_str(DEFAULT_ORG_ID).expect("valid default org id")
}

fn bearer_email(bearer: Option<&str>, state: &AppState) -> Option<String> {
    bearer.and_then(|token| {
        freebuff_shared::auth::decode_access_token(token, &state.config.jwt_secret)
            .ok()
            .map(|claims| claims.email)
    })
}

/// Extract the bearer token string from the optional typed auth header.
fn bearer_token(bearer: &Option<TypedHeader<Authorization<Bearer>>>) -> Option<&str> {
    bearer
        .as_ref()
        .map(|TypedHeader(Authorization(b))| b.token())
}

#[derive(Debug, Deserialize)]
pub struct UsageQuery {
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
}

pub async fn get_account(
    State(state): State<AppState>,
    bearer: Option<TypedHeader<Authorization<Bearer>>>,
) -> Result<Json<ApiResponse<BillingAccountPublic>>, AppError> {
    let org_id = resolve_org(&state, bearer_token(&bearer));
    let account = crate::db::billing::ensure_billing_account(&state.db, org_id).await?;
    Ok(Json(ApiResponse::new(account.into())))
}

pub async fn create_checkout_session(
    State(state): State<AppState>,
    bearer: Option<TypedHeader<Authorization<Bearer>>>,
    Json(input): Json<StripeSessionRequest>,
) -> Result<Json<ApiResponse<StripeSessionResponse>>, AppError> {
    if !state.config.stripe_enabled() {
        return Err(AppError::BadRequest(
            "Stripe billing is not configured (STRIPE_SECRET_KEY is empty)".into(),
        ));
    }
    if state.config.stripe_price_id.is_empty() {
        return Err(AppError::BadRequest(
            "STRIPE_PRICE_ID is not configured".into(),
        ));
    }

    let org_id = resolve_org(&state, bearer_token(&bearer));
    let email = bearer_email(bearer_token(&bearer), &state);
    let account = crate::db::billing::ensure_billing_account(&state.db, org_id).await?;

    // Create a Stripe customer on first checkout.
    let customer_id = match account.stripe_customer_id {
        Some(ref id) if !id.is_empty() => id.clone(),
        _ => {
            let email = email.as_deref().unwrap_or("billing@freebuff.local");
            let id = crate::services::stripe_service::create_customer(
                &state.config,
                &state.http,
                email,
                "Freebuff Org",
            )
            .await?;
            crate::db::billing::set_stripe_customer(&state.db, org_id, &id, Some(email))
                .await?;
            id
        }
    };

    let base_url = input
        .success_url
        .clone()
        .unwrap_or_else(|| "http://localhost:3000/billing?checkout=success".into());
    let cancel_url = input
        .cancel_url
        .clone()
        .unwrap_or_else(|| "http://localhost:3000/billing?checkout=canceled".into());

    let url = crate::services::stripe_service::create_checkout_session(
        &state.config,
        &state.http,
        &customer_id,
        &state.config.stripe_price_id,
        &base_url,
        &cancel_url,
    )
    .await?;

    Ok(Json(ApiResponse::new(StripeSessionResponse { url })))
}

pub async fn create_portal_session(
    State(state): State<AppState>,
    bearer: Option<TypedHeader<Authorization<Bearer>>>,
    Json(input): Json<StripeSessionRequest>,
) -> Result<Json<ApiResponse<StripeSessionResponse>>, AppError> {
    if !state.config.stripe_enabled() {
        return Err(AppError::BadRequest(
            "Stripe billing is not configured (STRIPE_SECRET_KEY is empty)".into(),
        ));
    }

    let org_id = resolve_org(&state, bearer_token(&bearer));
    let account = crate::db::billing::ensure_billing_account(&state.db, org_id).await?;

    let customer_id = account.stripe_customer_id.as_deref().unwrap_or_default();
    if customer_id.is_empty() {
        return Err(AppError::BadRequest(
            "No Stripe customer exists for this organization yet".into(),
        ));
    }

    let return_url = input
        .return_url
        .clone()
        .unwrap_or_else(|| "http://localhost:3000/billing".into());

    let url = crate::services::stripe_service::create_portal_session(
        &state.config,
        &state.http,
        customer_id,
        &return_url,
    )
    .await?;

    Ok(Json(ApiResponse::new(StripeSessionResponse { url })))
}

pub async fn cancel_subscription(
    State(state): State<AppState>,
    bearer: Option<TypedHeader<Authorization<Bearer>>>,
) -> Result<Json<ApiResponse<BillingAccountPublic>>, AppError> {
    let org_id = resolve_org(&state, bearer_token(&bearer));
    let account = crate::db::billing::ensure_billing_account(&state.db, org_id).await?;

    let subscription_id = account.stripe_subscription_id.as_deref().unwrap_or_default();
    if subscription_id.is_empty() {
        return Err(AppError::BadRequest("No active subscription".into()));
    }

    crate::services::stripe_service::cancel_subscription(
        &state.config,
        &state.http,
        subscription_id,
    )
    .await?;
    let account = crate::db::billing::set_billing_status(&state.db, org_id, "canceled").await?;

    Ok(Json(ApiResponse::new(account.into())))
}

pub async fn get_usage(
    State(state): State<AppState>,
    bearer: Option<TypedHeader<Authorization<Bearer>>>,
    Query(query): Query<UsageQuery>,
) -> Result<Json<ApiResponse<UsageSummary>>, AppError> {
    let org_id = resolve_org(&state, bearer_token(&bearer));

    let now = Utc::now();
    let period_start = query.from.unwrap_or_else(|| {
        now.date_naive()
            .with_day(1)
            .unwrap_or_else(|| now.date_naive())
            .and_hms_opt(0, 0, 0)
            .map(|dt| dt.and_utc())
            .unwrap_or(now)
    });
    let period_end = query.to.unwrap_or(now);

    let rows = crate::db::billing::usage_summary_rows(&state.db, org_id, period_start, period_end)
        .await?;

    let mut meters: Vec<MeterUsage> = Vec::new();
    for row in rows {
        match meters.iter_mut().find(|m| m.meter == row.meter) {
            Some(meter) => {
                meter.total += row.value;
                meter.daily.push(DailyUsage {
                    date: row.day,
                    value: row.value,
                });
            }
            None => meters.push(MeterUsage {
                meter: row.meter,
                total: row.value,
                daily: vec![DailyUsage {
                    date: row.day,
                    value: row.value,
                }],
            }),
        }
    }

    Ok(Json(ApiResponse::new(UsageSummary {
        period_start,
        period_end,
        meters,
    })))
}

pub async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<MessageResponse>, AppError> {
    if state.config.stripe_webhook_secret.is_empty() {
        return Err(AppError::Unauthorized(
            "STRIPE_WEBHOOK_SECRET is not configured".into(),
        ));
    }

    let signature = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("Missing Stripe-Signature header".into()))?;

    crate::services::stripe_service::verify_webhook_signature(
        &body,
        signature,
        &state.config.stripe_webhook_secret,
    )?;

    let event: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| AppError::BadRequest(format!("Invalid webhook payload: {}", e)))?;

    let event_id = event
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let event_type = event
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    // Idempotency: ignore already-processed events.
    let is_new = crate::db::billing::record_webhook_event(
        &state.db,
        event_id,
        event_type,
        &event,
    )
    .await?;

    if is_new {
        crate::services::stripe_service::handle_webhook(&state, &event).await?;
    } else {
        tracing::debug!("Ignoring duplicate Stripe webhook event {}", event_id);
    }

    Ok(Json(MessageResponse {
        message: "received".into(),
    }))
}