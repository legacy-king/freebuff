use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};

use crate::GatewayState;

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(_state): State<GatewayState>,
) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    tracing::info!("New WebSocket connection");

    while let Some(msg) = socket.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                tracing::debug!("Received message: {}", text);

                // In production, this would:
                // 1. Parse the subscription request
                // 2. Set up logical replication listener
                // 3. Stream changes back to the client

                if let Err(e) = socket
                    .send(Message::Text(
                        serde_json::json!({
                            "event": "connected",
                            "payload": {
                                "message": "Real-time connection established"
                            }
                        })
                        .to_string()
                        .into(),
                    ))
                    .await
                {
                    tracing::error!("Failed to send message: {}", e);
                    break;
                }
            }
            Ok(Message::Close(_)) => {
                tracing::info!("WebSocket connection closed");
                break;
            }
            Err(e) => {
                tracing::error!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }
}
