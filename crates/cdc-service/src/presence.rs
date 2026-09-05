use axum::{extract::{Path, State}, Json};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::CdcState;

/// Tracks which users are "present" on each channel.
///
/// Presence enables collaborative features like:
/// - Showing who's viewing a page
/// - Typing indicators
/// - Cursor positions
/// - Online/offline status
#[derive(Debug)]
pub struct PresenceManager {
    /// channel_key -> (user_key -> PresenceEntry)
    channels: DashMap<String, DashMap<String, PresenceEntry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceEntry {
    pub key: String,
    pub user: Option<serde_json::Value>,
    pub state: serde_json::Value,
    pub joined_at: chrono::DateTime<chrono::Utc>,
    pub last_seen: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct PresenceState {
    pub channel: String,
    pub presences: Vec<PresenceEntry>,
    pub count: usize,
}

impl PresenceManager {
    pub fn new() -> Self {
        Self {
            channels: DashMap::new(),
        }
    }

    /// Join a channel with presence state
    pub async fn join(
        &self,
        project_id: &str,
        channel: &str,
        key: &str,
        state: serde_json::Value,
    ) {
        let channel_key = format!("{}.{}", project_id, channel);
        let now = chrono::Utc::now();

        let entry = PresenceEntry {
            key: key.to_string(),
            user: None,
            state,
            joined_at: now,
            last_seen: now,
        };

        self.channels
            .entry(channel_key)
            .or_insert_with(DashMap::new)
            .insert(key.to_string(), entry);

        tracing::debug!("Presence join: {} on {}", key, channel);
    }

    /// Leave a channel
    pub async fn leave(&self, project_id: &str, channel: &str, key: &str) {
        let channel_key = format!("{}.{}", project_id, channel);

        if let Some(channel_map) = self.channels.get(&channel_key) {
            channel_map.remove(key);

            // Clean up empty channels
            if channel_map.is_empty() {
                drop(channel_map);
                self.channels.remove(&channel_key);
            }
        }

        tracing::debug!("Presence leave: {} from {}", key, channel);
    }

    /// Get all presences on a channel
    pub async fn get_presences(
        &self,
        project_id: &str,
        channel: &str,
    ) -> Vec<PresenceEntry> {
        let channel_key = format!("{}.{}", project_id, channel);

        self.channels
            .get(&channel_key)
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| entry.value().clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get presence count for a channel
    pub async fn count(&self, project_id: &str, channel: &str) -> usize {
        let channel_key = format!("{}.{}", project_id, channel);

        self.channels
            .get(&channel_key)
            .map(|entries| entries.len())
            .unwrap_or(0)
    }

    /// Update presence state
    pub async fn update_state(
        &self,
        project_id: &str,
        channel: &str,
        key: &str,
        new_state: serde_json::Value,
    ) -> bool {
        let channel_key = format!("{}.{}", project_id, channel);

        if let Some(channel_map) = self.channels.get(&channel_key) {
            if let Some(mut entry) = channel_map.get_mut(key) {
                entry.state = new_state;
                entry.last_seen = chrono::Utc::now();
                return true;
            }
        }

        false
    }

    /// Remove all presences for a project (cleanup)
    pub async fn cleanup_project(&self, project_id: &str) {
        let prefix = format!("{}.", project_id);
        self.channels.retain(|key, _| !key.starts_with(&prefix));
    }
}

/// Get presence for a channel
pub async fn get_presence(
    State(state): State<CdcState>,
    Path((project_id, channel)): Path<(String, String)>,
) -> Result<Json<PresenceState>, freebuff_shared::AppError> {
    let presences = state.presence.get_presences(&project_id, &channel).await;
    let count = presences.len();

    Ok(Json(PresenceState {
        channel,
        presences,
        count,
    }))
}

#[derive(Debug, Deserialize)]
pub struct JoinPresenceRequest {
    pub key: String,
    pub state: serde_json::Value,
    pub user: Option<serde_json::Value>,
}

/// Join presence on a channel
pub async fn join_presence(
    State(state): State<CdcState>,
    Path((project_id, channel)): Path<(String, String)>,
    Json(input): Json<JoinPresenceRequest>,
) -> Result<Json<serde_json::Value>, freebuff_shared::AppError> {
    state.presence.join(
        &project_id,
        &channel,
        &input.key,
        input.state.clone(),
    ).await;

    // Notify others via WebSocket
    let connections = state.connections.read().await;
    if let Some(conns) = connections.get(&project_id) {
        let presence_msg = crate::websocket::WsMessage::Presence {
            channel: channel.clone(),
            joins: vec![crate::websocket::PresenceState {
                key: input.key.clone(),
                user: input.user,
                state: input.state,
            }],
            leaves: vec![],
        };

        for conn in conns {
            let _ = conn.sender.send(presence_msg.clone()).await;
        }
    }

    let count = state.presence.count(&project_id, &channel).await;

    Ok(Json(serde_json::json!({
        "status": "joined",
        "key": input.key,
        "count": count,
    })))
}

#[derive(Debug, Deserialize)]
pub struct LeavePresenceRequest {
    pub key: String,
}

/// Leave presence on a channel
pub async fn leave_presence(
    State(state): State<CdcState>,
    Path((project_id, channel)): Path<(String, String)>,
    Json(input): Json<LeavePresenceRequest>,
) -> Result<Json<serde_json::Value>, freebuff_shared::AppError> {
    state.presence.leave(&project_id, &channel, &input.key).await;

    // Notify others via WebSocket
    let connections = state.connections.read().await;
    if let Some(conns) = connections.get(&project_id) {
        let presence_msg = crate::websocket::WsMessage::Presence {
            channel,
            joins: vec![],
            leaves: vec![input.key],
        };

        for conn in conns {
            let _ = conn.sender.send(presence_msg.clone()).await;
        }
    }

    Ok(Json(serde_json::json!({
        "status": "left",
    })))
}
