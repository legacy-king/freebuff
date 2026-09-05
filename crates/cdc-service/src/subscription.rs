use axum::{extract::{Path, State, Query}, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::change_event::{ChangeEvent, SubscriptionFilter};
use crate::CdcState;

/// A subscription represents a client's interest in database changes.
///
/// Subscriptions can filter by:
/// - Table name
/// - Event type (INSERT, UPDATE, DELETE)
/// - Column values
///
/// Events matching the filter are sent to the subscriber's channel.
#[derive(Debug, Clone)]
pub struct Subscription {
    /// Unique subscription ID
    pub id: String,

    /// The channel/topic name (e.g., "public:users" or "realtime:*")
    pub channel: String,

    /// Project this subscription belongs to
    pub project_id: String,

    /// Filter criteria
    pub filter: SubscriptionFilter,

    /// Sender for pushing events to this subscriber
    pub sender: mpsc::Sender<ChangeEvent>,

    /// When this subscription was created
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Optional: connection ID for tracking
    pub connection_id: Option<String>,

    /// Optional: user claims for RLS enforcement
    pub user_claims: Option<serde_json::Value>,
}

impl Subscription {
    /// Create a new subscription
    pub fn new(
        channel: &str,
        project_id: &str,
        filter: SubscriptionFilter,
        sender: mpsc::Sender<ChangeEvent>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            channel: channel.to_string(),
            project_id: project_id.to_string(),
            filter,
            sender,
            created_at: chrono::Utc::now(),
            connection_id: None,
            user_claims: None,
        }
    }

    /// Check if a change event matches this subscription's filter
    pub fn matches_event(&self, event: &ChangeEvent) -> bool {
        event.matches_filter(&self.filter)
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateSubscriptionRequest {
    /// Channel name (e.g., "public:users" or "realtime:*")
    pub channel: String,

    /// Filter criteria
    pub filter: Option<SubscriptionFilter>,

    /// Optional: connection ID
    pub connection_id: Option<String>,

    /// Optional: user claims for RLS
    pub user_claims: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionResponse {
    pub id: String,
    pub channel: String,
    pub project_id: String,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Parse a channel name into schema and table components
///
/// Channel formats:
/// - "schema:table" — specific table
/// - "schema:*" — all tables in schema
/// - "realtime:*" — all tables (legacy Supabase format)
/// - "table" — table in public schema
pub fn parse_channel(channel: &str) -> SubscriptionFilter {
    let parts: Vec<&str> = channel.split(':').collect();

    match parts.len() {
        1 => {
            // Just table name
            SubscriptionFilter {
                schema: Some("public".to_string()),
                table: Some(parts[0].to_string()),
                event: None,
                columns: None,
            }
        }
        2 => {
            let schema = parts[0];
            let table = parts[1];

            if table == "*" {
                SubscriptionFilter {
                    schema: Some(schema.to_string()),
                    table: None,
                    event: None,
                    columns: None,
                }
            } else {
                SubscriptionFilter {
                    schema: Some(schema.to_string()),
                    table: Some(table.to_string()),
                    event: None,
                    columns: None,
                }
            }
        }
        _ => {
            // Invalid format, subscribe to everything
            SubscriptionFilter {
                schema: None,
                table: None,
                event: None,
                columns: None,
            }
        }
    }
}

/// Create a subscription via REST API
pub async fn create_subscription(
    State(state): State<CdcState>,
    Path(project_id): Path<String>,
    Json(input): Json<CreateSubscriptionRequest>,
) -> Result<Json<SubscriptionResponse>, freebuff_shared::AppError> {
    let filter = input.filter.unwrap_or_else(|| parse_channel(&input.channel));

    // Create a channel for events
    let (sender, _receiver) = mpsc::channel(1000);

    let subscription = Subscription::new(
        &input.channel,
        &project_id,
        filter,
        sender,
    );

    let sub_id = subscription.id.clone();
    let created_at = subscription.created_at;

    // Add to subscriptions map
    let mut subs = state.subscriptions.write().await;
    let key = format!("{}.{}", project_id, input.channel);
    subs.entry(key)
        .or_insert_with(Vec::new)
        .push(subscription);

    tracing::info!(
        "Created subscription {} for project {} channel {}",
        sub_id,
        project_id,
        input.channel
    );

    Ok(Json(SubscriptionResponse {
        id: sub_id,
        channel: input.channel,
        project_id,
        status: "active".into(),
        created_at,
    }))
}

/// Remove a subscription via REST API
pub async fn remove_subscription(
    State(state): State<CdcState>,
    Path((project_id, channel)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, freebuff_shared::AppError> {
    let mut subs = state.subscriptions.write().await;
    let key = format!("{}.{}", project_id, channel);

    if let Some(channel_subs) = subs.get_mut(&key) {
        let before = channel_subs.len();
        channel_subs.retain(|s| s.channel != channel);
        let removed = before - channel_subs.len();

        if channel_subs.is_empty() {
            subs.remove(&key);
        }

        tracing::info!(
            "Removed {} subscription(s) for project {} channel {}",
            removed,
            project_id,
            channel
        );

        return Ok(Json(serde_json::json!({
            "status": "removed",
            "count": removed,
        })));
    }

    Ok(Json(serde_json::json!({
        "status": "not_found",
        "count": 0,
    })))
}
