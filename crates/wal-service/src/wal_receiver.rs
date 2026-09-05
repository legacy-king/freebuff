use axum::{extract::{Path, State}, Json};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{WalState, LSN};
use freebuff_shared::AppError;

/// Request from a PostgreSQL primary to send WAL records.
/// This is called by the `wal_receiver_status_interval` callback or
/// via a custom WAL sender in Postgres.
#[derive(Debug, Deserialize)]
pub struct WalReceiveRequest {
    /// The starting LSN of the WAL data being sent
    pub start_lsn: String,
    // The WAL data (binary) — in practice this comes from the HTTP body
}

#[derive(Debug, Serialize)]
pub struct WalReceiveResponse {
    /// The LSN up to which WAL was received
    pub end_lsn: String,
    /// Number of bytes received
    pub bytes_received: usize,
    /// Whether the timeline is in sync
    pub synced: bool,
}

/// Receive WAL data from a PostgreSQL primary.
/// This endpoint is called by our custom WAL sender module in PostgreSQL.
///
/// In production, this would be a streaming protocol (like pg_basebackup
/// or a custom WAL sender). For now, we accept batched WAL via HTTP POST.
pub async fn receive_wal(
    State(state): State<WalState>,
    Path(timeline_id): Path<String>,
    body: Bytes,
) -> Result<Json<WalReceiveResponse>, AppError> {
    if body.is_empty() {
        return Err(AppError::BadRequest("Empty WAL data".into()));
    }

    tracing::debug!(
        "Receiving {} bytes of WAL for timeline {}",
        body.len(),
        timeline_id
    );

    let mut timelines = state.timelines.write().await;

    let timeline = timelines
        .get_mut(&timeline_id)
        .ok_or_else(|| AppError::NotFound(format!("Timeline {} not found", timeline_id)))?;

    // Get the current LSN as the start point
    let start_lsn = timeline.current_lsn();

    // Append WAL data (Arc must be unique while the write lock is held)
    let timeline_mut = Arc::get_mut(timeline).ok_or_else(|| {
        AppError::Internal("Timeline is currently shared; cannot append WAL".into())
    })?;
    let end_lsn = timeline_mut.append_wal(&body, start_lsn);

    tracing::debug!(
        "Timeline {}: WAL appended from {} to {} ({} bytes)",
        timeline_id,
        start_lsn,
        end_lsn,
        body.len()
    );

    Ok(Json(WalReceiveResponse {
        end_lsn: end_lsn.to_pg_string(),
        bytes_received: body.len(),
        synced: true,
    }))
}

/// Stream WAL from a PostgreSQL primary using the replication protocol.
/// This connects to Postgres as a replication client and receives WAL in real-time.
pub async fn start_wal_streaming(
    timeline_id: String,
    primary_connstring: String,
    timelines: Arc<tokio::sync::RwLock<std::collections::HashMap<String, Arc<crate::Timeline>>>>,
) -> anyhow::Result<()> {
    tracing::info!(
        "Starting WAL streaming for timeline {} from {}",
        timeline_id,
        primary_connstring
    );

    // Connect to the primary as a replication client
    let (client, mut connection) = tokio_postgres::connect(
        &primary_connstring,
        tokio_postgres::NoTls,
    ).await?;

    // Handle the connection on a background task
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::error!("WAL streaming connection error: {}", e);
        }
    });

    // Start logical replication
    let replication_slot = format!("freebuff_{}", timeline_id);

    // Create replication slot if it doesn't exist
    client.execute(
        &format!("CREATE_REPLICATION_SLOT IF NOT EXISTS {} LOGICAL pg_output", replication_slot),
        &[],
    ).await?;

    // Start streaming
    let mut stream = Box::pin(
        client
            .copy_out(&format!(
                "START_REPLICATION LOGICAL {} (\"proto_version\" '1', \"publication_names\" 'freebuff publication')",
                replication_slot
            ))
            .await?,
    );

    tracing::info!("WAL streaming started for timeline {}", timeline_id);

    // Read WAL data from the stream
    while let Some(data) = stream.next().await {
        match data {
            Ok(bytes) => {
                let mut timelines_guard = timelines.write().await;
                if let Some(timeline) = timelines_guard.get_mut(&timeline_id) {
                    if let Some(timeline_mut) = Arc::get_mut(timeline) {
                        let start_lsn = timeline_mut.current_lsn();
                        timeline_mut.append_wal(&bytes, start_lsn);
                    }
                }
            }
            Err(e) => {
                tracing::error!("WAL streaming error for {}: {}", timeline_id, e);
                break;
            }
        }
    }

    tracing::info!("WAL streaming ended for timeline {}", timeline_id);
    Ok(())
}

use futures::StreamExt;
