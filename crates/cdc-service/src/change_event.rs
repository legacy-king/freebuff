use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single change event from PostgreSQL logical replication.
///
/// This represents an INSERT, UPDATE, DELETE, or TRUNCATE operation
/// on a monitored table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeEvent {
    /// Unique identifier for this change event
    pub id: String,

    /// The schema name (usually "public")
    pub schema: String,

    /// The table name that was modified
    pub table: String,

    /// The type of operation
    pub op: ChangeOperation,

    /// The old data (before change) — present for UPDATE and DELETE
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<ColumnData>,

    /// The new data (after change) — present for INSERT and UPDATE
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<ColumnData>,

    /// The LSN (Log Sequence Number) when this change occurred
    pub lsn: String,

    /// The timestamp when this change was captured
    pub timestamp: DateTime<Utc>,

    /// The project this change belongs to
    pub project_id: String,

    /// Column metadata (types, constraints)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<ColumnMetadata>>,

    /// Commit timestamp from PostgreSQL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_timestamp: Option<DateTime<Utc>>,
}

/// The type of database operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum ChangeOperation {
    INSERT,
    UPDATE,
    DELETE,
    TRUNCATE,
}

impl std::fmt::Display for ChangeOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::INSERT => write!(f, "INSERT"),
            Self::UPDATE => write!(f, "UPDATE"),
            Self::DELETE => write!(f, "DELETE"),
            Self::TRUNCATE => write!(f, "TRUNCATE"),
        }
    }
}

/// Column data as a map of column name to value
pub type ColumnData = serde_json::Map<String, serde_json::Value>;

/// A single column value with its name and type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnValue {
    pub name: String,
    pub value: serde_json::Value,
    #[serde(rename = "type")]
    pub column_type: String,
}

/// Column metadata from the table schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnMetadata {
    pub name: String,
    #[serde(rename = "type")]
    pub column_type: String,
    pub nullable: bool,
    pub is_primary_key: bool,
    pub default_value: Option<String>,
}

/// Parsed logical replication message from PostgreSQL
#[derive(Debug, Clone)]
pub enum ReplicationMessage {
    /// BEGIN <LSN>
    Begin {
        lsn: String,
        commit_time: i64,
    },
    /// RELATION <id> <schema> <table> <columns>
    Relation {
        id: u32,
        schema: String,
        table: String,
        columns: Vec<ColumnInfo>,
    },
    /// INSERT <id> <data>
    Insert {
        relation_id: u32,
        row_data: Vec<Option<String>>,
    },
    /// UPDATE <id> [<old_data>] <data>
    Update {
        relation_id: u32,
        old_row_data: Option<Vec<Option<String>>>,
        row_data: Vec<Option<String>>,
    },
    /// DELETE <id> <data>
    Delete {
        relation_id: u32,
        old_row_data: Vec<Option<String>>,
    },
    /// TRUNCATE <id>
    Truncate {
        relation_ids: Vec<u32>,
    },
    /// COMMIT <LSN>
    Commit {
        lsn: String,
        commit_time: i64,
    },
    /// ORIGIN <origin_id> <lsn>
    Origin {
        origin_id: i64,
        lsn: String,
    },
    /// Type <id> <namespace> <name>
    Type {
        id: u32,
        namespace: String,
        name: String,
    },
}

#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub flags: u8,
    pub name: String,
    pub type_id: u32,
    pub type_modifier: i32,
}

impl ChangeEvent {
    /// Create a new change event
    pub fn new(
        schema: &str,
        table: &str,
        op: ChangeOperation,
        lsn: &str,
        project_id: &str,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            schema: schema.to_string(),
            table: table.to_string(),
            op,
            old: None,
            new: None,
            lsn: lsn.to_string(),
            timestamp: Utc::now(),
            project_id: project_id.to_string(),
            columns: None,
            commit_timestamp: None,
        }
    }

    /// Get the primary key columns from column metadata
    pub fn primary_key_columns(&self) -> Vec<&str> {
        self.columns
            .as_ref()
            .map(|cols| {
                cols.iter()
                    .filter(|c| c.is_primary_key)
                    .map(|c| c.name.as_str())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get the primary key values from row data
    pub fn primary_key_values(&self, data: &ColumnData) -> serde_json::Value {
        let pk_cols = self.primary_key_columns();
        if pk_cols.is_empty() {
            return serde_json::Value::Null;
        }

        let mut pk = serde_json::Map::new();
        for col in &pk_cols {
            if let Some(val) = data.get(*col) {
                pk.insert(col.to_string(), val.clone());
            }
        }
        serde_json::Value::Object(pk)
    }

    /// Convert to a compact format for WebSocket transmission
    pub fn to_websocket_payload(&self) -> serde_json::Value {
        let mut payload = serde_json::json!({
            "id": self.id,
            "schema": self.schema,
            "table": self.table,
            "op": self.op,
            "lsn": self.lsn,
            "timestamp": self.timestamp,
        });

        if let Some(ref old) = self.old {
            payload["old"] = serde_json::Value::Object(old.clone());
        }

        if let Some(ref new) = self.new {
            payload["new"] = serde_json::Value::Object(new.clone());
        }

        payload
    }

    /// Check if this event matches a subscription filter
    pub fn matches_filter(&self, filter: &SubscriptionFilter) -> bool {
        // Check schema filter
        if let Some(ref schema_filter) = filter.schema {
            if &self.schema != schema_filter {
                return false;
            }
        }

        // Check table filter
        if let Some(ref table_filter) = filter.table {
            if &self.table != table_filter {
                return false;
            }
        }

        // Check event type filter
        if let Some(ref event_filter) = filter.event {
            if &self.op.to_string() != event_filter {
                return false;
            }
        }

        // Check column filters
        if let Some(ref column_filters) = filter.columns {
            let data = self.new.as_ref().or(self.old.as_ref());
            if let Some(row_data) = data {
                for col_filter in column_filters {
                    if let Some(value) = row_data.get(&col_filter.column) {
                        match &col_filter.condition {
                            FilterCondition::Equals(expected) => {
                                if value != expected {
                                    return false;
                                }
                            }
                            FilterCondition::NotEquals(expected) => {
                                if value == expected {
                                    return false;
                                }
                            }
                            FilterCondition::GreaterThan(expected) => {
                                if let (Some(val_num), Some(exp_num)) = (
                                    value.as_f64(),
                                    expected.as_f64(),
                                ) {
                                    if val_num <= exp_num {
                                        return false;
                                    }
                                }
                            }
                            FilterCondition::LessThan(expected) => {
                                if let (Some(val_num), Some(exp_num)) = (
                                    value.as_f64(),
                                    expected.as_f64(),
                                ) {
                                    if val_num >= exp_num {
                                        return false;
                                    }
                                }
                            }
                            FilterCondition::Contains(expected) => {
                                if let Some(s) = value.as_str() {
                                    if !s.contains(expected) {
                                        return false;
                                    }
                                } else {
                                    return false;
                                }
                            }
                        }
                    } else {
                        return false;
                    }
                }
            }
        }

        true
    }
}

/// Filter condition for column-based filtering
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operator", content = "value")]
pub enum FilterCondition {
    #[serde(rename = "eq")]
    Equals(serde_json::Value),
    #[serde(rename = "neq")]
    NotEquals(serde_json::Value),
    #[serde(rename = "gt")]
    GreaterThan(serde_json::Value),
    #[serde(rename = "gte")]
    GreaterThanOrEqual(serde_json::Value),
    #[serde(rename = "lt")]
    LessThan(serde_json::Value),
    #[serde(rename = "lte")]
    LessThanOrEqual(serde_json::Value),
    #[serde(rename = "like")]
    Contains(String),
}

/// Filter for subscriptions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionFilter {
    pub schema: Option<String>,
    pub table: Option<String>,
    pub event: Option<String>,
    pub columns: Option<Vec<ColumnFilter>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnFilter {
    pub column: String,
    #[serde(flatten)]
    pub condition: FilterCondition,
}
