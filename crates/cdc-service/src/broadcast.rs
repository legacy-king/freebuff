use axum::{extract::{Path, State, Query}, Json};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::CdcState;
use crate::websocket::WsMessage;

/// Broadcast channels enable pub/sub messaging without database changes.
///
/// This is useful for:
/// - Application-level notifications
/// - Custom events
/// - Chat messages
/// - Typing indicators
#[derive(Debug)]
pub struct BroadcastManager {
    /// channel_key -> list of subscribers (connection_ids)
    channels: DashMap<String, Vec<BroadcastSubscriber>>,
}

#[derive(Debug, Clone)]
struct BroadcastSubscriber {
    connection_id: String,
    events: Option<Vec<String>>, // If None, receives all events
}

#[derive(Debug, Serialize)]
pub struct BroadcastResponse {
    pub status: String,
    pub channel: String,
    pub message_id: String,
}

impl BroadcastManager {
    pub fn new() -> Self {
        Self {
            channels: DashMap::new(),
        }
    }

    /// Send a broadcast message to all subscribers on a channel
    pub async fn send(
        &self,
        project_id: &str,
        channel: &str,
        payload: serde_json::Value,
        event: Option<String>,
    ) -> String {
        let channel_key = format!("{}.{}", project_id, channel);
        let message_id = uuid::Uuid::new_v4().to_string();

        let broadcast_msg = WsMessage::Broadcast {
            channel: channel.to_string(),
            payload,
            event: event.clone(),
        };

        if let Some(subscribers) = self.channels.get(&channel_key) {
            // In a real implementation, we'd look up connection senders
            // and filter by event type
            tracing::debug!(
                "Broadcast to {} subscribers on {}",
                subscribers.len(),
                channel_key
            );
        }

        message_id
    }

    /// Subscribe a connection to a broadcast channel
    pub fn subscribe(
        &self,
        project_id: &str,
        channel: &str,
        connection_id: &str,
        events: Option<Vec<String>>,
    ) {
        let channel_key = format!("{}.{}", project_id, channel);

        let subscriber = BroadcastSubscriber {
            connection_id: connection_id.to_string(),
            events,
        };

        self.channels
            .entry(channel_key)
            .or_insert_with(Vec::new)
            .push(subscriber);
    }

    /// Unsubscribe a connection from a broadcast channel
    pub fn unsubscribe(
        &self,
        project_id: &str,
        channel: &str,
        connection_id: &str,
    ) {
        let channel_key = format!("{}.{}", project_id, channel);

        if let Some(mut subscribers) = self.channels.get_mut(&channel_key) {
            subscribers.retain(|s| s.connection_id != connection_id);

            if subscribers.is_empty() {
                drop(subscribers);
                self.channels.remove(&channel_key);
            }
        }
    }

    /// Get subscriber count for a channel
    pub fn subscriber_count(&self, project_id: &str, channel: &str) -> usize {
        let channel_key = format!("{}.{}", project_id, channel);

        self.channels
            .get(&channel_key)
            .map(|s| s.len())
            .unwrap_or(0)
    }

    /// Clean up all subscriptions for a connection
    pub fn cleanup_connection(&self, connection_id: &str) {
        for mut entry in self.channels.iter_mut() {
            entry.retain(|s| s.connection_id != connection_id);
        }

        // Remove empty channels
        self.channels.retain(|_, subscribers| !subscribers.is_empty());
    }
}

#[derive(Debug, Deserialize)]
pub struct BroadcastRequest {
    /// The payload to broadcast
    pub payload: serde_json::Value,

    /// Optional event name (for filtering on client side)
    pub event: Option<String>,

    /// Optional: only broadcast to specific connection IDs
    pub to: Option<Vec<String>>,
}

/// Send a broadcast message via REST API
pub async fn send_broadcast(
    State(state): State<CdcState>,
    Path((project_id, channel)): Path<(String, String)>,
    Json(input): Json<BroadcastRequest>,
) -> Result<Json<BroadcastResponse>, freebuff_shared::AppError> {
    let message_id = uuid::Uuid::new_v4().to_string();

    // Create broadcast message
    let broadcast_msg = WsMessage::Broadcast {
        channel: channel.clone(),
        payload: input.payload,
        event: input.event.clone(),
    };

    // Send to all connections on this channel
    let connections = state.connections.read().await;
    if let Some(conns) = connections.get(&project_id) {
        let mut sent = 0;

        for conn in conns {
            // If 'to' filter is specified, only send to those connections
            if let Some(ref to_filter) = input.to {
                if !to_filter.contains(&conn.id) {
                    continue;
                }
            }

            let _ = conn.sender.send(broadcast_msg.clone()).await;
            sent += 1;
        }

        tracing::debug!(
            "Broadcast to {} connections on {}.{}",
            sent,
            project_id,
            channel
        );
    }

    Ok(Json(BroadcastResponse {
        status: "ok".into(),
        channel,
        message_id,
    }))
}
