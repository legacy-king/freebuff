use serde::{Deserialize, Serialize};

/// Client -> Server messages
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    /// Subscribe to database changes on a channel
    #[serde(rename = "subscribe")]
    Subscribe {
        channel: String,
        filter: Option<ChannelFilter>,
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

    /// Join presence on a channel
    #[serde(rename = "presence")]
    Presence {
        channel: String,
        #[serde(flatten)]
        action: PresenceAction,
    },

    /// Heartbeat response (pong)
    #[serde(rename = "pong")]
    Pong {},
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action")]
pub enum PresenceAction {
    #[serde(rename = "join")]
    Join {
        key: String,
        state: serde_json::Value,
    },

    #[serde(rename = "leave")]
    Leave {
        key: String,
    },

    #[serde(rename = "track")]
    Track {
        key: String,
        state: serde_json::Value,
    },
}

#[derive(Debug, Deserialize)]
pub struct ChannelFilter {
    /// Filter by event type: INSERT, UPDATE, DELETE
    pub event: Option<String>,

    /// Filter by specific columns
    pub columns: Option<Vec<ColumnFilter>>,

    /// Filter by schema (default: public)
    pub schema: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ColumnFilter {
    pub column: String,
    #[serde(flatten)]
    pub condition: FilterOp,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op")]
pub enum FilterOp {
    #[serde(rename = "eq")]
    Eq(serde_json::Value),
    #[serde(rename = "neq")]
    Neq(serde_json::Value),
    #[serde(rename = "gt")]
    Gt(serde_json::Value),
    #[serde(rename = "gte")]
    Gte(serde_json::Value),
    #[serde(rename = "lt")]
    Lt(serde_json::Value),
    #[serde(rename = "lte")]
    Lte(serde_json::Value),
    #[serde(rename = "in")]
    In(Vec<serde_json::Value>),
}

/// Server -> Client messages
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    /// Connection established
    #[serde(rename = "welcome")]
    Welcome {
        connection_id: String,
        project_id: String,
        version: String,
    },

    /// Subscription confirmed
    #[serde(rename = "subscribe")]
    SubscribeOk {
        channel: String,
        status: String,
        subscription_id: String,
    },

    /// Subscription error
    #[serde(rename = "subscribe_error")]
    SubscribeError {
        channel: String,
        message: String,
    },

    /// Database change event
    #[serde(rename = "changes")]
    Changes {
        channel: String,
        events: Vec<ChangePayload>,
    },

    /// Presence update
    #[serde(rename = "presence")]
    Presence {
        channel: String,
        joins: Vec<PresenceUpdate>,
        leaves: Vec<String>,
    },

    /// Broadcast message
    #[serde(rename = "broadcast")]
    Broadcast {
        channel: String,
        payload: serde_json::Value,
        event: Option<String>,
        message_id: String,
    },

    /// Heartbeat request
    #[serde(rename = "heartbeat")]
    Ping {
        timestamp: chrono::DateTime<chrono::Utc>,
    },

    /// Error
    #[serde(rename = "error")]
    Error {
        message: String,
        code: Option<String>,
    },
}

#[derive(Debug, Serialize)]
pub struct ChangePayload {
    pub id: String,
    pub schema: String,
    pub table: String,
    pub op: String,
    pub old: Option<serde_json::Value>,
    pub new: Option<serde_json::Value>,
    pub lsn: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct PresenceUpdate {
    pub key: String,
    pub user: Option<serde_json::Value>,
    pub state: serde_json::Value,
}

/// Convert a ChangeEvent to the WebSocket protocol format
pub fn change_event_to_payload(
    event: &crate::change_event::ChangeEvent,
) -> ChangePayload {
    ChangePayload {
        id: event.id.clone(),
        schema: event.schema.clone(),
        table: event.table.clone(),
        op: event.op.to_string(),
        old: event.old.as_ref().map(|o| serde_json::Value::Object(o.clone())),
        new: event.new.as_ref().map(|n| serde_json::Value::Object(n.clone())),
        lsn: event.lsn.clone(),
        timestamp: event.timestamp,
    }
}
