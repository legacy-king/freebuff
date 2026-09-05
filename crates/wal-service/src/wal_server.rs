use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{timeline::{Timeline, TimelineState, LSN}, WalState};
use freebuff_shared::AppError;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub timeline_count: usize,
}

pub async fn health_check(
    State(state): State<WalState>,
) -> Json<HealthResponse> {
    let timelines = state.timelines.read().await;
    Json(HealthResponse {
        status: "healthy".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        timeline_count: timelines.len(),
    })
}

// ── Timeline Management ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateTimelineRequest {
    pub timeline_id: String,
    pub parent_id: Option<String>,
    pub parent_branch_lsn: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TimelineResponse {
    pub id: String,
    pub parent_id: Option<String>,
    pub parent_branch_lsn: Option<String>,
    pub state: String,
    pub current_lsn: String,
    pub segment_count: usize,
    pub total_wal_bytes: usize,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn create_timeline(
    State(state): State<WalState>,
    Json(input): Json<CreateTimelineRequest>,
) -> Result<Json<TimelineResponse>, AppError> {
    let parent_branch_lsn = input.parent_branch_lsn
        .as_deref()
        .map(LSN::from_str)
        .transpose()
        .map_err(|e| AppError::BadRequest(e))?;

    let data_dir = std::path::PathBuf::from(&state.config.wal_dir)
        .join(&input.timeline_id);

    std::fs::create_dir_all(&data_dir)
        .map_err(|e| AppError::Internal(format!("Failed to create data dir: {}", e)))?;

    let timeline = Arc::new(Timeline::new(
        input.timeline_id.clone(),
        input.parent_id.clone(),
        parent_branch_lsn,
        data_dir,
    ));

    let mut timelines = state.timelines.write().await;
    timelines.insert(input.timeline_id.clone(), timeline.clone());

    tracing::info!("Created timeline {}", input.timeline_id);

    Ok(Json(TimelineResponse {
        id: timeline.id.clone(),
        parent_id: timeline.parent_id.clone(),
        parent_branch_lsn: timeline.parent_branch_lsn.map(|l| l.to_pg_string()),
        state: format!("{:?}", timeline.state),
        current_lsn: timeline.current_lsn().to_pg_string(),
        segment_count: timeline.segment_count(),
        total_wal_bytes: timeline.total_wal_bytes(),
        created_at: timeline.created_at,
    }))
}

pub async fn get_timeline(
    State(state): State<WalState>,
    Path(timeline_id): Path<String>,
) -> Result<Json<TimelineResponse>, AppError> {
    let timelines = state.timelines.read().await;

    let timeline = timelines
        .get(&timeline_id)
        .ok_or_else(|| AppError::NotFound(format!("Timeline {} not found", timeline_id)))?;

    Ok(Json(TimelineResponse {
        id: timeline.id.clone(),
        parent_id: timeline.parent_id.clone(),
        parent_branch_lsn: timeline.parent_branch_lsn.map(|l| l.to_pg_string()),
        state: format!("{:?}", timeline.state),
        current_lsn: timeline.current_lsn().to_pg_string(),
        segment_count: timeline.segment_count(),
        total_wal_bytes: timeline.total_wal_bytes(),
        created_at: timeline.created_at,
    }))
}

#[derive(Debug, Serialize)]
pub struct TimelineStatusResponse {
    pub id: String,
    pub state: String,
    pub current_lsn: String,
    pub segments: Vec<SegmentInfo>,
}

#[derive(Debug, Serialize)]
pub struct SegmentInfo {
    pub segment_id: u64,
    pub start_lsn: String,
    pub end_lsn: String,
    pub size_bytes: usize,
    pub archived: bool,
}

pub async fn timeline_status(
    State(state): State<WalState>,
    Path(timeline_id): Path<String>,
) -> Result<Json<TimelineStatusResponse>, AppError> {
    let timelines = state.timelines.read().await;

    let timeline = timelines
        .get(&timeline_id)
        .ok_or_else(|| AppError::NotFound(format!("Timeline {} not found", timeline_id)))?;

    let segments = timeline.segments.iter().map(|(_, seg)| SegmentInfo {
        segment_id: seg.segment_id,
        start_lsn: seg.start_lsn.to_pg_string(),
        end_lsn: seg.end_lsn.to_pg_string(),
        size_bytes: seg.data.len(),
        archived: seg.archived,
    }).collect();

    Ok(Json(TimelineStatusResponse {
        id: timeline.id.clone(),
        state: format!("{:?}", timeline.state),
        current_lsn: timeline.current_lsn().to_pg_string(),
        segments,
    }))
}

// ── WAL Serving ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ServeWalQuery {
    pub max_bytes: Option<usize>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ServeWalResponse {
    pub start_lsn: String,
    pub end_lsn: String,
    pub data_base64: String,
    pub bytes_served: usize,
}

pub async fn serve_wal(
    State(state): State<WalState>,
    Path((timeline_id, start_lsn)): Path<(String, String)>,
    Query(query): Query<ServeWalQuery>,
) -> Result<Json<ServeWalResponse>, AppError> {
    let start = LSN::from_str(&start_lsn)
        .map_err(|e| AppError::BadRequest(e))?;

    let timelines = state.timelines.read().await;

    let timeline = timelines
        .get(&timeline_id)
        .ok_or_else(|| AppError::NotFound(format!("Timeline {} not found", timeline_id)))?;

    // Wait for new WAL data if we're caught up
    let max_bytes = query.max_bytes.unwrap_or(1024 * 1024); // 1MB default

    if start >= timeline.current_lsn() {
        // We're caught up — wait for new data or timeout
        let timeout = std::time::Duration::from_millis(query.timeout_ms.unwrap_or(1000));

        // Drop the read lock before waiting
        drop(timelines);

        // Wait for new WAL or timeout
        let timelines = state.timelines.read().await;
        if let Some(timeline) = timelines.get(&timeline_id) {
            let _ = tokio::time::timeout(timeout, timeline.write_notify.notified()).await;
        }

        // Re-acquire and serve
        let timelines = state.timelines.read().await;
        let timeline = timelines.get(&timeline_id)
            .ok_or_else(|| AppError::NotFound("Timeline disappeared".into()))?;

        let wal_data = timeline.get_wal_from(start);
        let end_lsn = if wal_data.is_empty() {
            start
        } else {
            LSN(start.0 + wal_data.len() as u64)
        };

        let truncated = if wal_data.len() > max_bytes {
            wal_data[..max_bytes].to_vec()
        } else {
            wal_data
        };

        return Ok(Json(ServeWalResponse {
            start_lsn: start.to_pg_string(),
            end_lsn: end_lsn.to_pg_string(),
            data_base64: base64_encode(&truncated),
            bytes_served: truncated.len(),
        }));
    }

    let wal_data = timeline.get_wal_from(start);
    let end_lsn = LSN(start.0 + wal_data.len() as u64);

    let truncated = if wal_data.len() > max_bytes {
        wal_data[..max_bytes].to_vec()
    } else {
        wal_data
    };

    Ok(Json(ServeWalResponse {
        start_lsn: start.to_pg_string(),
        end_lsn: end_lsn.to_pg_string(),
        data_base64: base64_encode(&truncated),
        bytes_served: truncated.len(),
    }))
}

// ── Point-in-Time Recovery ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RestorePitrRequest {
    /// Target LSN to restore to (exclusive — restore up to but not including this LSN)
    pub target_lsn: String,
    /// Base backup data (base64-encoded tar)
    pub base_backup: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RestorePitrResponse {
    pub timeline_id: String,
    pub restored_to_lsn: String,
    pub wal_bytes_restored: usize,
    pub status: String,
}

pub async fn restore_pitr(
    State(state): State<WalState>,
    Path(timeline_id): Path<String>,
    Json(input): Json<RestorePitrRequest>,
) -> Result<Json<RestorePitrResponse>, AppError> {
    let target = LSN::from_str(&input.target_lsn)
        .map_err(|e| AppError::BadRequest(e))?;

    tracing::info!(
        "PITR restore for timeline {} to LSN {}",
        timeline_id,
        target
    );

    // Get archived segments from S3
    let segments = state.archiver.list_segments(&timeline_id)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to list segments: {}", e)))?;

    if segments.is_empty() {
        return Err(AppError::BadRequest("No archived WAL segments found".into()));
    }

    // Find the base segment (first segment)
    let base_segment = *segments.first().unwrap();

    // Restore WAL up to target LSN
    let wal_data = state.archiver.restore_to_lsn(&timeline_id, base_segment, target)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to restore WAL: {}", e)))?;

    tracing::info!(
        "PITR complete: restored {} bytes of WAL to {}",
        wal_data.len(),
        target
    );

    Ok(Json(RestorePitrResponse {
        timeline_id,
        restored_to_lsn: target.to_pg_string(),
        wal_bytes_restored: wal_data.len(),
        status: "restored".into(),
    }))
}

// ── Branch Timeline Creation ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateBranchTimelineRequest {
    /// New timeline ID for the branch
    pub branch_timeline_id: String,
    /// Parent timeline to branch from
    pub parent_timeline_id: String,
    /// LSN to branch at (if None, branches from current LSN)
    pub branch_lsn: Option<String>,
}

pub async fn create_branch_timeline(
    State(state): State<WalState>,
    Json(input): Json<CreateBranchTimelineRequest>,
) -> Result<Json<TimelineResponse>, AppError> {
    let branch_lsn = input.branch_lsn
        .as_deref()
        .map(LSN::from_str)
        .transpose()
        .map_err(|e| AppError::BadRequest(e))?;

    let timelines = state.timelines.read().await;

    let parent = timelines
        .get(&input.parent_timeline_id)
        .ok_or_else(|| AppError::NotFound(format!("Parent timeline {} not found", input.parent_timeline_id)))?;

    // The branch point LSN
    let fork_lsn = branch_lsn.unwrap_or(parent.current_lsn());

    // Create new timeline data directory
    let data_dir = std::path::PathBuf::from(&state.config.wal_dir)
        .join(&input.branch_timeline_id);

    std::fs::create_dir_all(&data_dir)
        .map_err(|e| AppError::Internal(format!("Failed to create data dir: {}", e)))?;

    drop(timelines);

    // Create the new timeline — it starts with the parent's WAL up to the fork point
    let new_timeline = Arc::new(Timeline::new(
        input.branch_timeline_id.clone(),
        Some(input.parent_timeline_id.clone()),
        Some(fork_lsn),
        data_dir.clone(),
    ));

    // Copy WAL segments up to the fork point from parent
    {
        let timelines = state.timelines.read().await;
        let parent = timelines.get(&input.parent_timeline_id).unwrap();

        let mut new_tl = Arc::try_unwrap(new_timeline).unwrap_or_else(|arc| (*arc).clone());
        for (&seg_id, segment) in &parent.segments {
            if segment.end_lsn <= fork_lsn {
                new_tl.segments.insert(seg_id, segment.clone());
            }
        }
        new_tl.current_lsn = fork_lsn;
        new_timeline = Arc::new(new_tl);
    }

    let mut timelines = state.timelines.write().await;
    timelines.insert(input.branch_timeline_id.clone(), new_timeline.clone());

    tracing::info!(
        "Created branch timeline {} from {} at LSN {}",
        input.branch_timeline_id,
        input.parent_timeline_id,
        fork_lsn
    );

    Ok(Json(TimelineResponse {
        id: new_timeline.id.clone(),
        parent_id: new_timeline.parent_id.clone(),
        parent_branch_lsn: new_timeline.parent_branch_lsn.map(|l| l.to_pg_string()),
        state: format!("{:?}", new_timeline.state),
        current_lsn: new_timeline.current_lsn().to_pg_string(),
        segment_count: new_timeline.segment_count(),
        total_wal_bytes: new_timeline.total_wal_bytes(),
        created_at: new_timeline.created_at,
    }))
}

// ── Helpers ────────────────────────────────────────────────────────────

fn base64_encode(data: &[u8]) -> String {
    use base64_encode_inner;
    // Simple base64 encoding
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

// Placeholder for base64 module
mod base64_encode_inner {}
