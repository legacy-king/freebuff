use anyhow::Result;
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_postgres::NoTls;

use crate::change_event::{
    ChangeEvent, ChangeOperation, ColumnData, ColumnInfo, ColumnMetadata,
    ReplicationMessage, SubscriptionFilter,
};
use crate::subscription::Subscription;

/// A logical replication client that reads WAL from PostgreSQL
/// and produces change events for the CDC pipeline.
pub struct ReplicationClient {
    /// The project this client is monitoring
    project_id: String,

    /// Connection string to the PostgreSQL instance
    connection_string: String,

    /// Channel for sending change events
    event_sender: mpsc::Sender<ChangeEvent>,

    /// The replication slot name
    slot_name: String,

    /// Publication name
    publication_name: String,

    /// Current LSN position
    current_lsn: Option<String>,

    /// Relation cache (relation_id -> table info)
    relations: HashMap<u32, RelationInfo>,
}

struct RelationInfo {
    schema: String,
    table: String,
    columns: Vec<ColumnInfo>,
}

impl ReplicationClient {
    /// Create a new replication client
    pub async fn new(
        database_url: &str,
        project_id: &str,
    ) -> Result<Self> {
        let slot_name = format!("freebuff_cdc_{}", project_id.replace('-', "_"));
        let publication_name = format!("freebuff_pub_{}", project_id.replace('-', "_"));

        // Connect and create replication slot if needed
        let (client, mut connection) = tokio_postgres::connect(database_url, NoTls).await?;

        // Handle connection on background task
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!("Replication connection error: {}", e);
            }
        });

        // Create publication for all tables
        client.execute(&format!(
            "CREATE PUBLICATION {} FOR ALL TABLES",
            publication_name
        ), &[]).await.ok(); // Ignore if already exists

        // Create replication slot
        client.execute(&format!(
            "CREATE_REPLICATION_SLOT IF NOT EXISTS {} LOGICAL pg_output",
            slot_name
        ), &[]).await?;

        tracing::info!(
            "Created replication slot '{}' and publication '{}'",
            slot_name,
            publication_name
        );

        let (event_sender, _receiver) = mpsc::channel(10000);

        Ok(Self {
            project_id: project_id.to_string(),
            connection_string: database_url.to_string(),
            event_sender,
            slot_name,
            publication_name,
            current_lsn: None,
            relations: HashMap::new(),
        })
    }

    /// Start the replication stream.
    ///
    /// This connects to PostgreSQL as a logical replication client and
    /// reads WAL records in real-time, converting them to ChangeEvents.
    pub async fn start_streaming(
        &mut self,
        subscriptions: Arc<tokio::sync::RwLock<HashMap<String, Vec<Subscription>>>>,
    ) -> Result<()> {
        tracing::info!(
            "Starting logical replication for project {}",
            self.project_id
        );

        let (client, mut connection) = tokio_postgres::connect(
            &self.connection_string,
            NoTls,
        ).await?;

        // Handle connection on background task
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!("Replication connection error: {}", e);
            }
        });

        // Start logical replication
        let start_lsn = self.current_lsn.as_deref().unwrap_or("0/0");

        let query = format!(
            "START_REPLICATION SLOT {} LOGICAL {} \
             (\"proto_version\" '1', \"publication_names\" '{}')",
            self.slot_name,
            start_lsn,
            self.publication_name
        );

        tracing::debug!("Starting replication: {}", query);

        let mut stream = Box::pin(client.copy_out(&query).await?);

        tracing::info!(
            "Replication stream started for project {} at LSN {}",
            self.project_id,
            start_lsn
        );

        // Read messages from the replication stream
        while let Some(data) = stream.next().await {
            match data {
                Ok(bytes) => {
                    // Parse the replication message
                    match self.parse_replication_message(&bytes) {
                        Ok(Some(events)) => {
                            for event in events {
                                // Broadcast to subscribers
                                self.broadcast_event(&event, &subscriptions).await;
                            }
                        }
                        Ok(None) => {
                            // Control message (keepalive, etc.)
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse replication message: {}", e);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Replication stream error: {}", e);
                    break;
                }
            }
        }

        tracing::info!("Replication stream ended for project {}", self.project_id);
        Ok(())
    }

    /// Parse a raw replication message into change events
    fn parse_replication_message(
        &mut self,
        data: &[u8],
    ) -> Result<Option<Vec<ChangeEvent>>> {
        if data.is_empty() {
            return Ok(None);
        }

        // PostgreSQL logical replication message format:
        // Byte 0: Message type identifier
        // Rest: Message-specific data

        let msg_type = data[0];
        let payload = &data[1..];

        match msg_type {
            // 'B' - BEGIN
            b'B' => {
                // Format: int64 (LSN), int64 (timestamp), int32 (xid)
                if payload.len() >= 20 {
                    let lsn = u64::from_be_bytes(payload[0..8].try_into()?);
                    let _commit_time = i64::from_be_bytes(payload[8..16].try_into()?);

                    self.current_lsn = Some(format!("{:X}/{:08X}", lsn >> 32, lsn & 0xFFFFFFFF));

                    tracing::trace!("BEGIN at LSN {}", self.current_lsn.as_deref().unwrap_or("?"));
                }
                Ok(None)
            }

            // 'R' - RELATION
            b'R' => {
                self.parse_relation_message(payload)?;
                Ok(None)
            }

            // 'I' - INSERT
            b'I' => {
                let event = self.parse_insert_message(payload)?;
                Ok(Some(vec![event]))
            }

            // 'U' - UPDATE
            b'U' => {
                let event = self.parse_update_message(payload)?;
                Ok(Some(vec![event]))
            }

            // 'D' - DELETE
            b'D' => {
                let event = self.parse_delete_message(payload)?;
                Ok(Some(vec![event]))
            }

            // 'T' - TRUNCATE
            b'T' => {
                let event = self.parse_truncate_message(payload)?;
                Ok(Some(vec![event]))
            }

            // 'C' - COMMIT
            b'C' => {
                if payload.len() >= 8 {
                    let lsn = u64::from_be_bytes(payload[0..8].try_into()?);
                    self.current_lsn = Some(format!("{:X}/{:08X}", lsn >> 32, lsn & 0xFFFFFFFF));
                    tracing::trace!("COMMIT at LSN {}", self.current_lsn.as_deref().unwrap_or("?"));
                }
                Ok(None)
            }

            // 'O' - ORIGIN
            b'O' => Ok(None),

            // 'Y' - TYPE
            b'Y' => Ok(None),

            // Keepalive message
            b'k' => {
                // Respond with keepalive
                tracing::trace!("Keepalive received");
                Ok(None)
            }

            _ => {
                tracing::debug!("Unknown replication message type: {}", msg_type);
                Ok(None)
            }
        }
    }

    /// Parse a RELATION message (table schema)
    fn parse_relation_message(&mut self, data: &[u8]) -> Result<()> {
        let mut pos = 0;

        // Relation ID (4 bytes)
        let relation_id = u32::from_be_bytes(data[pos..pos + 4].try_into()?);
        pos += 4;

        // Namespace (null-terminated string)
        let (namespace, new_pos) = read_cstring(data, pos)?;
        pos = new_pos;

        // Relation name (null-terminated string)
        let (relation_name, new_pos) = read_cstring(data, pos)?;
        pos = new_pos;

        // Replica identity (1 byte)
        pos += 1;

        // Column count (2 bytes)
        let num_columns = u16::from_be_bytes(data[pos..pos + 2].try_into()?) as usize;
        pos += 2;

        let mut columns = Vec::new();

        for _ in 0..num_columns {
            if pos >= data.len() {
                break;
            }

            // Column flags (1 byte)
            let flags = data[pos];
            pos += 1;

            // Column name (null-terminated string)
            let (col_name, new_pos) = read_cstring(data, pos)?;
            pos = new_pos;

            // Type OID (4 bytes)
            let type_id = u32::from_be_bytes(data[pos..pos + 4].try_into()?);
            pos += 4;

            // Type modifier (4 bytes)
            let type_modifier = i32::from_be_bytes(data[pos..pos + 4].try_into()?);
            pos += 4;

            columns.push(ColumnInfo {
                flags,
                name: col_name,
                type_id,
                type_modifier,
            });
        }

        self.relations.insert(relation_id, RelationInfo {
            schema: namespace,
            table: relation_name,
            columns,
        });

        Ok(())
    }

    /// Parse an INSERT message
    fn parse_insert_message(&self, data: &[u8]) -> Result<ChangeEvent> {
        let mut pos = 0;

        // Relation ID (4 bytes)
        let relation_id = u32::from_be_bytes(data[pos..pos + 4].try_into()?);
        pos += 4;

        // New tuple data
        let row_data = self.parse_tuple_data(&data[pos..])?;

        let relation = self.relations.get(&relation_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown relation ID: {}", relation_id))?;

        let mut event = ChangeEvent::new(
            &relation.schema,
            &relation.table,
            ChangeOperation::INSERT,
            self.current_lsn.as_deref().unwrap_or("0/0"),
            &self.project_id,
        );

        // Convert row data to column data
        let mut new_data = ColumnData::new();
        for (i, value) in row_data.iter().enumerate() {
            if let Some(col) = relation.columns.get(i) {
                let json_value = convert_column_value(value.as_deref(), col.type_id);
                new_data.insert(col.name.clone(), json_value);
            }
        }
        event.new = Some(new_data);

        // Set column metadata
        event.columns = Some(relation.columns.iter().map(|c| ColumnMetadata {
            name: c.name.clone(),
            column_type: format_type_oid(c.type_id),
            nullable: true,
            is_primary_key: c.flags & 1 != 0,
            default_value: None,
        }).collect());

        Ok(event)
    }

    /// Parse an UPDATE message
    fn parse_update_message(&self, data: &[u8]) -> Result<ChangeEvent> {
        let mut pos = 0;

        // Relation ID (4 bytes)
        let relation_id = u32::from_be_bytes(data[pos..pos + 4].try_into()?);
        pos += 4;

        let relation = self.relations.get(&relation_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown relation ID: {}", relation_id))?;

        // Check for old tuple (key unchanged)
        let first_byte = data[pos];
        pos += 1;

        let (old_data, new_data) = match first_byte {
            b'K' => {
                // Old key data present
                let old_key = self.parse_tuple_data(&data[pos..])?;
                let key_len = calculate_tuple_data_length(&data[pos..]);
                pos += key_len;
                pos += 1; // New tuple indicator
                let new_tuple = self.parse_tuple_data(&data[pos..])?;
                (Some(old_key), new_tuple)
            }
            b'O' => {
                // Old row data present
                let old_row = self.parse_tuple_data(&data[pos..])?;
                let row_len = calculate_tuple_data_length(&data[pos..]);
                pos += row_len;
                pos += 1; // New tuple indicator
                let new_tuple = self.parse_tuple_data(&data[pos..])?;
                (Some(old_row), new_tuple)
            }
            _ => {
                // No old data, just new tuple
                let new_tuple = self.parse_tuple_data(&data[pos..])?;
                (None, new_tuple)
            }
        };

        let mut event = ChangeEvent::new(
            &relation.schema,
            &relation.table,
            ChangeOperation::UPDATE,
            self.current_lsn.as_deref().unwrap_or("0/0"),
            &self.project_id,
        );

        // Convert old data
        if let Some(old_row) = old_data {
            let mut old = ColumnData::new();
            for (i, value) in old_row.iter().enumerate() {
                if let Some(col) = relation.columns.get(i) {
                    let json_value = convert_column_value(value.as_deref(), col.type_id);
                    old.insert(col.name.clone(), json_value);
                }
            }
            event.old = Some(old);
        }

        // Convert new data
        let mut new = ColumnData::new();
        for (i, value) in new_data.iter().enumerate() {
            if let Some(col) = relation.columns.get(i) {
                let json_value = convert_column_value(value.as_deref(), col.type_id);
                new.insert(col.name.clone(), json_value);
            }
        }
        event.new = Some(new);

        Ok(event)
    }

    /// Parse a DELETE message
    fn parse_delete_message(&self, data: &[u8]) -> Result<ChangeEvent> {
        let mut pos = 0;

        // Relation ID (4 bytes)
        let relation_id = u32::from_be_bytes(data[pos..pos + 4].try_into()?);
        pos += 4;

        // Check for old tuple type
        let first_byte = data[pos];
        pos += 1;

        let old_row = match first_byte {
            b'K' | b'O' => self.parse_tuple_data(&data[pos..])?,
            _ => Vec::new(),
        };

        let relation = self.relations.get(&relation_id)
            .ok_or_else(|| anyhow::anyhow!("Unknown relation ID: {}", relation_id))?;

        let mut event = ChangeEvent::new(
            &relation.schema,
            &relation.table,
            ChangeOperation::DELETE,
            self.current_lsn.as_deref().unwrap_or("0/0"),
            &self.project_id,
        );

        // Convert old data
        let mut old = ColumnData::new();
        for (i, value) in old_row.iter().enumerate() {
            if let Some(col) = relation.columns.get(i) {
                let json_value = convert_column_value(value.as_deref(), col.type_id);
                old.insert(col.name.clone(), json_value);
            }
        }
        event.old = Some(old);

        Ok(event)
    }

    /// Parse a TRUNCATE message
    fn parse_truncate_message(&self, data: &[u8]) -> Result<ChangeEvent> {
        // TRUNCATE messages contain a count of relations and their IDs
        // For simplicity, we'll return a generic truncate event
        Ok(ChangeEvent::new(
            "public",
            "*",
            ChangeOperation::TRUNCATE,
            self.current_lsn.as_deref().unwrap_or("0/0"),
            &self.project_id,
        ))
    }

    /// Parse tuple data from replication message
    fn parse_tuple_data(&self, data: &[u8]) -> Result<Vec<Option<String>>> {
        let mut values = Vec::new();
        let mut pos = 0;

        if pos >= data.len() {
            return Ok(values);
        }

        // Column count (2 bytes)
        let num_columns = u16::from_be_bytes(data[pos..pos + 2].try_into()?) as usize;
        pos += 2;

        for _ in 0..num_columns {
            if pos >= data.len() {
                break;
            }

            // Column type (1 byte)
            let col_type = data[pos];
            pos += 1;

            match col_type {
                b'n' => {
                    // NULL value
                    values.push(None);
                }
                b'u' => {
                    // TOASTed value (unchanged)
                    values.push(Some("TOASTED".to_string()));
                }
                b't' => {
                    // Text value
                    let len = i32::from_be_bytes(data[pos..pos + 4].try_into()?) as usize;
                    pos += 4;
                    let text = String::from_utf8_lossy(&data[pos..pos + len]).to_string();
                    pos += len;
                    values.push(Some(text));
                }
                b'b' => {
                    // Binary value
                    let len = i32::from_be_bytes(data[pos..pos + 4].try_into()?) as usize;
                    pos += 4;
                    pos += len; // Skip binary data
                    values.push(Some(format!("binary_{}bytes", len)));
                }
                _ => {
                    // Unknown type
                    values.push(None);
                }
            }
        }

        Ok(values)
    }

    /// Broadcast an event to all matching subscribers
    async fn broadcast_event(
        &self,
        event: &ChangeEvent,
        subscriptions: &Arc<tokio::sync::RwLock<HashMap<String, Vec<Subscription>>>>,
    ) {
        let subs = subscriptions.read().await;

        let table_key = format!("{}.{}", event.project_id, event.table);
        let all_key = format!("{}.*", event.project_id);

        // Check table-specific subscriptions
        if let Some(table_subs) = subs.get(&table_key) {
            for sub in table_subs {
                if sub.matches_event(event) {
                    let _ = sub.sender.send(event.clone()).await;
                }
            }
        }

        // Check wildcard subscriptions
        if let Some(all_subs) = subs.get(&all_key) {
            for sub in all_subs {
                if sub.matches_event(event) {
                    let _ = sub.sender.send(event.clone()).await;
                }
            }
        }
    }
}

/// Read a null-terminated C string from a byte buffer
fn read_cstring(data: &[u8], start: usize) -> Result<(String, usize)> {
    let mut end = start;
    while end < data.len() && data[end] != 0 {
        end += 1;
    }
    let s = String::from_utf8_lossy(&data[start..end]).to_string();
    Ok((s, end + 1)) // +1 to skip the null terminator
}

/// Calculate the byte length of tuple data
fn calculate_tuple_data_length(data: &[u8]) -> usize {
    if data.len() < 2 {
        return 0;
    }

    let num_columns = u16::from_be_bytes(data[0..2].try_into().unwrap_or([0, 0])) as usize;
    let mut pos = 2;

    for _ in 0..num_columns {
        if pos >= data.len() {
            break;
        }

        match data[pos] {
            b'n' | b'u' => pos += 1,
            b't' | b'b' => {
                pos += 1;
                if pos + 4 <= data.len() {
                    let len = i32::from_be_bytes(data[pos..pos + 4].try_into().unwrap_or([0; 4])) as usize;
                    pos += 4 + len;
                } else {
                    break;
                }
            }
            _ => pos += 1,
        }
    }

    pos
}

/// Convert a text column value to a JSON value based on its type OID
fn convert_column_value(value: Option<&str>, type_oid: u32) -> serde_json::Value {
    match value {
        None => serde_json::Value::Null,
        Some(v) => {
            match type_oid {
                // int2, int4, int8
                21 | 23 | 20 => {
                    v.parse::<i64>()
                        .map(serde_json::Value::from)
                        .unwrap_or_else(|_| serde_json::Value::String(v.to_string()))
                }
                // float4, float8, numeric
                700 | 701 | 1700 => {
                    v.parse::<f64>()
                        .map(serde_json::Value::from)
                        .unwrap_or_else(|_| serde_json::Value::String(v.to_string()))
                }
                // bool
                16 => {
                    serde_json::Value::Bool(v == "t" || v == "true")
                }
                // uuid
                2950 => {
                    serde_json::Value::String(v.to_string())
                }
                // json, jsonb
                114 | 3802 => {
                    serde_json::from_str(v)
                        .unwrap_or_else(|_| serde_json::Value::String(v.to_string()))
                }
                // timestamp, timestamptz
                1114 | 1184 => {
                    serde_json::Value::String(v.to_string())
                }
                // Everything else as string
                _ => {
                    serde_json::Value::String(v.to_string())
                }
            }
        }
    }
}

/// Format a PostgreSQL type OID to a human-readable name
fn format_type_oid(oid: u32) -> String {
    match oid {
        16 => "bool".into(),
        17 => "bytea".into(),
        18 => "char".into(),
        19 => "name".into(),
        20 => "int8".into(),
        21 => "int2".into(),
        22 => "int2vector".into(),
        23 => "int4".into(),
        24 => "regproc".into(),
        25 => "text".into(),
        26 => "oid".into(),
        700 => "float4".into(),
        701 => "float8".into(),
        1114 => "timestamp".into(),
        1184 => "timestamptz".into(),
        114 => "json".into(),
        1700 => "numeric".into(),
        2950 => "uuid".into(),
        3802 => "jsonb".into(),
        _ => format!("oid_{}", oid),
    }
}
