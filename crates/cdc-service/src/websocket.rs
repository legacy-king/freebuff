use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::change_event::ChangeEvent;
use crate::subscription::{parse_channel, Subscription};
use crate::CdcState;

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    /// API key for authentication
    pub api_key: Option<String>,

    /// Project ID
    pub project_id: Option<String>,

    /// Heartbeat interval in seconds
    pub heartbeat: Option<u64>,
}

/// A WebSocket connection to the CDC service.
pub struct WsConnection {
    pub id: String,
    pub project_id: String,
    pub sender: mpsc::Sender<WsMessage>,
}

impl WsConnection {
    pub async fn send_heartbeat(&self) -> anyhow::Result<()> {
        let msg = WsMessage::Heartbeat {
            timestamp: chrono::Utc::now(),
        };
        self.sender.send(msg).await?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    #[serde(rename = "heartbeat")]
    Heartbeat {
        timestamp: chrono::DateTime<chrono::Utc>,
    },

    #[serde(rename = "changes")]
    Changes {
        channel: String,
        events: Vec<serde_json::Value>,
    },

    #[serde(rename = "presence")]
    Presence {
        channel: String,
        joins: Vec<PresenceState>,
        leaves: Vec<String>,
    },

    #[serde(rename = "broadcast")]
    Broadcast {
        channel: String,
        payload: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        event: Option<String>,
    },

    #[serde(rename = "error")]
    Error {
        message: String,
        code: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct PresenceState {
    pub key: String,
    pub user: Option<serde_json::Value>,
    pub state: serde_json::Value,
}

/// Handle a WebSocket upgrade request
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<CdcState>,
) -> impl IntoResponse {
    let project_id = query.project_id.unwrap_or_else(|| "default".into());

    ws.on_upgrade(move |socket| handle_socket(socket, project_id, state))
}

/// Handle an individual WebSocket connection
async fn handle_socket(socket: WebSocket, project_id: String, state: CdcState) {
    let connection_id = Uuid::new_v4().to_string();
    tracing::info!(
        "WebSocket connection opened: {} (project: {})",
        connection_id,
        project_id
    );

    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (event_tx, mut event_rx) = mpsc::channel::<WsMessage>(100);

    // Create connection entry
    let ws_conn = Arc::new(WsConnection {
        id: connection_id.clone(),
        project_id: project_id.clone(),
        sender: event_tx,
    });

    // Register connection
    {
        let mut connections = state.connections.write().await;
        connections
            .entry(project_id.clone())
            .or_insert_with(Vec::new)
            .push(ws_conn.clone());
    }

    // Send connection established message
    let welcome = serde_json::json!({
        "type": "welcome",
        "connection_id": connection_id,
        "project_id": project_id,
        "version": "1.0.0",
    });
    let _ = ws_sender.send(Message::Text(welcome.to_string().into())).await;

    // Spawn task to forward events to WebSocket
    let mut ws_sender_clone = ws_sender;
    let forward_task = tokio::spawn(async move {
        while let Some(msg) = event_rx.recv().await {
            match serde_json::to_string(&msg) {
                Ok(json) => {
                    if ws_sender_clone.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to serialize message: {}", e);
                }
            }
        }
    });

    // Handle incoming messages
    while let Some(result) = ws_receiver.next().await {
        match result {
            Ok(Message::Text(text)) => {
                let text_str: &str = &text;
                match serde_json::from_str::<WsClientMessage>(text_str) {
                    Ok(client_msg) => {
                        handle_client_message(
                            &connection_id,
                            &project_id,
                            client_msg,
                            &state,
                        ).await;
                    }
                    Err(e) => {
                        tracing::debug!("Failed to parse client message: {}", e);
                    }
                }
            }
            Ok(Message::Ping(data)) => {
                let _ = ws_sender.send(Message::Pong(data)).await;
            }
            Ok(Message::Close(_)) => {
                break;
            }
            Err(e) => {
                tracing::debug!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    // Cleanup
    tracing::info!("WebSocket connection closed: {}", connection_id);

    // Remove connection
    {
        let mut connections = state.connections.write().await;
        if let Some(conns) = connections.get_mut(&project_id) {
            conns.retain(|c| c.id != connection_id);
            if conns.is_empty() {
                connections.remove(&project_id);
            }
        }
    }

    // Remove subscriptions for this connection
    {
        let mut subs = state.subscriptions.write().await;
        for channel_subs in subs.values_mut() {
            channel_subs.retain(|s| s.connection_id.as_deref() != Some(&connection_id));
        }
    }

    forward_task.abort();
}

/// Messages from the WebSocket client
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum WsClientMessage {
    /// Subscribe to a channel
    #[serde(rename = "subscribe")]
    Subscribe {
        channel: String,
        filter: Option<crate::change_event::SubscriptionFilter>,
    },

    /// Unsubscribe from a channel
    #[serde(rename = "unsubscribe")]
    Unsubscribe {
        channel: String,
    },

    /// Send a broadcast message
    #[serde(rename = "broadcast")]
    Broadcast {
        channel: String,
        payload: serde_json::Value,
        event: Option<String>,
    },

    /// Join presence
    #[serde(rename = "presence_join")]
    PresenceJoin {
        channel: String,
        key: String,
        state: serde_json::Value,
    },

    /// Leave presence
    #[serde(rename = "presence_leave")]
    PresenceLeave {
        channel: String,
        key: String,
    },

    /// Heartbeat response
    #[serde(rename = "heartbeat")]
    HeartbeatResponse {},
}

/// Handle a message from the WebSocket client
async fn handle_client_message(
    connection_id: &str,
    project_id: &str,
    message: WsClientMessage,
    state: &CdcState,
) {
    match message {
        WsClientMessage::Subscribe { channel, filter } => {
            tracing::debug!(
                "Connection {} subscribing to {}",
                connection_id,
                channel
            );

            let sub_filter = filter.unwrap_or_else(|| parse_channel(&channel));

            // Create event channel
            let (event_tx, mut event_rx) = mpsc::channel::<ChangeEvent>(1000);

            // Get the connection's message sender
            let conn_sender = {
                let connections = state.connections.read().await;
                connections.get(project_id)
                    .and_then(|conns| conns.iter().find(|c| c.id == connection_id).cloned())
            };

            if let Some(conn) = conn_sender {
                // Spawn task to forward events to WebSocket
                let sender = conn.sender.clone();
                let channel_clone = channel.clone();

                tokio::spawn(async move {
                    while let Some(event) = event_rx.recv().await {
                        let payload = serde_json::json!({
                            "type": "changes",
                            "channel": channel_clone,
                            "event": event.op,
                            "schema": event.schema,
                            "table": event.table,
                            "data": event.to_websocket_payload(),
                        });
                        let _ = sender.send(WsMessage::Changes {
                            channel: channel_clone.clone(),
                            events: vec![payload],
                        }).await;
                    }
                });

                // Create and store subscription
                let subscription = Subscription::new(
                    &channel,
                    project_id,
                    sub_filter,
                    event_tx,
                );

                let sub_id = subscription.id.clone();

                let mut subs = state.subscriptions.write().await;
                let key = format!("{}.{}", project_id, channel);
                subs.entry(key)
                    .or_insert_with(Vec::new)
                    .push(subscription);

                // Send confirmation
                let confirmation = serde_json::json!({
                    "type": "subscribe",
                    "channel": channel,
                    "status": "ok",
                    "subscription_id": sub_id,
                });
                let _ = conn.sender.send(WsMessage::Changes {
                    channel,
                    events: vec![confirmation],
                }).await;
            }
        }

        WsClientMessage::Unsubscribe { channel } => {
            tracing::debug!(
                "Connection {} unsubscribing from {}",
                connection_id,
                channel
            );

            let mut subs = state.subscriptions.write().await;
            let key = format!("{}.{}", project_id, channel);

            if let Some(channel_subs) = subs.get_mut(&key) {
                channel_subs.retain(|s| s.connection_id.as_deref() != Some(connection_id));
                if channel_subs.is_empty() {
                    subs.remove(&key);
                }
            }
        }

        WsClientMessage::Broadcast { channel, payload, event } => {
            tracing::debug!(
                "Broadcast on channel {} from connection {}",
                channel,
                connection_id
            );

            let broadcast_msg = WsMessage::Broadcast {
                channel: channel.clone(),
                payload,
                event,
            };

            // Send to all connections on this channel
            let connections = state.connections.read().await;
            if let Some(conns) = connections.get(project_id) {
                for conn in conns {
                    if conn.id != connection_id {
                        let _ = conn.sender.send(broadcast_msg.clone()).await;
                    }
                }
            }
        }

        WsClientMessage::PresenceJoin { channel, key, state: presence_state } => {
            state.presence.join(
                project_id,
                &channel,
                &key,
                presence_state,
            ).await;

            // Notify others
            let connections = state.connections.read().await;
            if let Some(conns) = connections.get(project_id) {
                let presence_msg = WsMessage::Presence {
                    channel: channel.clone(),
                    joins: vec![crate::websocket::PresenceState {
                        key,
                        user: None,
                        state: presence_state,
                    }],
                    leaves: vec![],
                };

                for conn in conns {
                    if conn.id != connection_id {
                        let _ = conn.sender.send(presence_msg.clone()).await;
                    }
                }
            }
        }

        WsClientMessage::PresenceLeave { channel, key } => {
            state.presence.leave(project_id, &channel, &key).await;

            // Notify others
            let connections = state.connections.read().await;
            if let Some(conns) = connections.get(project_id) {
                let presence_msg = WsMessage::Presence {
                    channel,
                    joins: vec![],
                    leaves: vec![key],
                };

                for conn in conns {
                    if conn.id != connection_id {
                        let _ = conn.sender.send(presence_msg.clone()).await;
                    }
                }
            }
        }

        WsClientMessage::HeartbeatResponse {} => {
            // Client acknowledged heartbeat
            tracing::trace!("Heartbeat acknowledged by {}", connection_id);
        }
    }
}
