use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use tokio::sync::Notify;

/// Log Sequence Number — a monotonic integer representing a point in WAL history.
/// Format: XX/XXXXXXXX (e.g., "0/15D6B38")
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LSN(pub u64);

impl LSN {
    pub fn from_raw(lsn: u64) -> Self {
        Self(lsn)
    }

    pub fn from_str(s: &str) -> Result<Self, String> {
        // Parse PostgreSQL LSN format: "0/15D6B38"
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() != 2 {
            return Err(format!("Invalid LSN format: {}", s));
        }
        let hi = u64::from_str_radix(parts[0], 16).map_err(|e| e.to_string())?;
        let lo = u64::from_str_radix(parts[1], 16).map_err(|e| e.to_string())?;
        Ok(Self((hi << 32) | lo))
    }

    pub fn to_pg_string(&self) -> String {
        format!("{:X}/{:08X}", self.0 >> 32, self.0 & 0xFFFFFFFF)
    }

    pub fn is_valid(&self) -> bool {
        self.0 > 0
    }
}

impl std::fmt::Display for LSN {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_pg_string())
    }
}

/// A WAL segment — 16MB chunk of WAL data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalSegment {
    pub timeline_id: String,
    pub segment_id: u64,
    pub start_lsn: LSN,
    pub end_lsn: LSN,
    pub data: Vec<u8>,
    pub archived: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl WalSegment {
    pub const SEGMENT_SIZE: usize = 16 * 1024 * 1024; // 16MB

    pub fn segment_id_for_lsn(lsn: LSN) -> u64 {
        lsn.0 / Self::SEGMENT_SIZE as u64
    }

    pub fn segment_start_lsn(segment_id: u64) -> LSN {
        LSN(segment_id * Self::SEGMENT_SIZE as u64)
    }
}

/// Timeline represents the WAL history of a single Postgres instance.
/// It tracks all WAL segments and enables branching at any LSN.
#[derive(Debug)]
pub struct Timeline {
    pub id: String,
    pub parent_id: Option<String>,
    pub parent_branch_lsn: Option<LSN>,
    pub state: TimelineState,
    pub segments: BTreeMap<u64, WalSegment>,
    pub current_lsn: LSN,
    pub write_notify: Notify,
    pub data_dir: PathBuf,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimelineState {
    /// Receiving WAL from a primary
    Streaming,
    /// Not actively receiving WAL but data is available
    Inactive,
    /// Being restored from archive
    Restoring,
    /// Timeline has been archived and is no longer active
    Archived,
    /// Error state
    Error(String),
}

impl Timeline {
    pub fn new(
        id: String,
        parent_id: Option<String>,
        parent_branch_lsn: Option<LSN>,
        data_dir: PathBuf,
    ) -> Self {
        Self {
            id: id.clone(),
            parent_id,
            parent_branch_lsn,
            state: TimelineState::Streaming,
            segments: BTreeMap::new(),
            current_lsn: LSN(0),
            write_notify: Notify::new(),
            data_dir,
            created_at: chrono::Utc::now(),
        }
    }

    /// Append WAL data to the timeline. Returns the new LSN.
    pub fn append_wal(&mut self, data: &[u8], start_lsn: LSN) -> LSN {
        let end_lsn = LSN(start_lsn.0 + data.len() as u64);
        let segment_id = WalSegment::segment_id_for_lsn(start_lsn);

        let segment = self.segments.entry(segment_id).or_insert_with(|| WalSegment {
            timeline_id: self.id.clone(),
            segment_id,
            start_lsn: WalSegment::segment_start_lsn(segment_id),
            end_lsn: start_lsn,
            data: Vec::new(),
            archived: false,
            created_at: chrono::Utc::now(),
        });

        // Append data to the segment
        segment.data.extend_from_slice(data);
        segment.end_lsn = end_lsn;

        self.current_lsn = end_lsn;

        // Wake up any waiters for new WAL
        self.write_notify.notify_waiters();

        end_lsn
    }

    /// Get WAL data from a given LSN
    pub fn get_wal_from(&self, start_lsn: LSN) -> Vec<u8> {
        let mut result = Vec::new();

        for (_, segment) in self.segments.range(
            WalSegment::segment_id_for_lsn(start_lsn)..
        ) {
            if segment.end_lsn > start_lsn {
                let offset = if segment.start_lsn > start_lsn {
                    0
                } else {
                    (start_lsn.0 - segment.start_lsn.0) as usize
                };

                if offset < segment.data.len() {
                    result.extend_from_slice(&segment.data[offset..]);
                }
            }
        }

        result
    }

    /// Get the current LSN
    pub fn current_lsn(&self) -> LSN {
        self.current_lsn
    }

    /// Get segment count
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Get total WAL size in bytes
    pub fn total_wal_bytes(&self) -> usize {
        self.segments.values().map(|s| s.data.len()).sum()
    }
}

impl Serialize for Timeline {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Timeline", 7)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("parent_id", &self.parent_id)?;
        state.serialize_field("parent_branch_lsn", &self.parent_branch_lsn)?;
        state.serialize_field("state", &self.state)?;
        state.serialize_field("current_lsn", &self.current_lsn.to_pg_string())?;
        state.serialize_field("segment_count", &self.segment_count())?;
        state.serialize_field("total_wal_bytes", &self.total_wal_bytes())?;
        state.serialize_field("created_at", &self.created_at)?;
        state.end()
    }
}
