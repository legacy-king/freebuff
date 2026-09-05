use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

use crate::{WalConfig, WalSegment, LSN};
use freebuff_shared::AppError;

/// WalArchiver handles archiving WAL segments to object storage (S3/MinIO).
///
/// The archiver:
/// 1. Periodically scans timelines for unarchived segments
/// 2. Compresses segments with zstd
/// 3. Uploads to S3 with a structured key path
/// 4. Marks segments as archived
///
/// Key path format: {timeline_id}/{segment_id:016x}.wal.zst
pub struct WalArchiver {
    config: WalConfig,
    s3_client: reqwest::Client,
}

impl WalArchiver {
    pub async fn new(config: WalConfig) -> anyhow::Result<Self> {
        let s3_client = reqwest::Client::new();

        tracing::info!("WAL archiver initialized (S3: {})", config.s3_endpoint);

        Ok(Self {
            config,
            s3_client,
        })
    }

    /// Archive a single WAL segment to S3
    pub async fn archive_segment(
        &self,
        timeline_id: &str,
        segment: &WalSegment,
    ) -> anyhow::Result<String> {
        let key = self.segment_key(timeline_id, segment.segment_id);

        // Compress the segment data with zstd
        let compressed = zstd::encode_all(&segment.data[..], 3)?;

        tracing::debug!(
            "Archiving segment {} for timeline {} ({} bytes -> {} bytes compressed)",
            segment.segment_id,
            timeline_id,
            segment.data.len(),
            compressed.len()
        );

        // Upload to S3
        let url = format!(
            "{}/{}",
            self.config.s3_endpoint,
            key
        );

        let response = self.s3_client
            .put(&url)
            .header("Content-Type", "application/octet-stream")
            .basic_auth(
                &self.config.s3_access_key,
                Some(&self.config.s3_secret_key),
            )
            .body(compressed)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("S3 upload failed: {} - {}", status, body);
        }

        tracing::info!("Archived WAL segment: {}", key);
        Ok(key)
    }

    /// Download and decompress a WAL segment from S3
    pub async fn restore_segment(
        &self,
        timeline_id: &str,
        segment_id: u64,
    ) -> anyhow::Result<Vec<u8>> {
        let key = self.segment_key(timeline_id, segment_id);

        tracing::debug!("Restoring WAL segment: {}", key);

        let url = format!(
            "{}/{}",
            self.config.s3_endpoint,
            key
        );

        let response = self.s3_client
            .get(&url)
            .basic_auth(
                &self.config.s3_access_key,
                Some(&self.config.s3_secret_key),
            )
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("S3 download failed: {}", response.status());
        }

        let compressed = response.bytes().await?;

        // Decompress
        let decompressed = zstd::decode_all(compressed.as_ref())?;

        Ok(decompressed)
    }

    /// Restore all WAL segments up to a target LSN for PITR
    pub async fn restore_to_lsn(
        &self,
        timeline_id: &str,
        base_segment_id: u64,
        target_lsn: LSN,
    ) -> anyhow::Result<Vec<u8>> {
        let target_segment_id = WalSegment::segment_id_for_lsn(target_lsn);
        let mut all_wal = Vec::new();

        for segment_id in base_segment_id..=target_segment_id {
            match self.restore_segment(timeline_id, segment_id).await {
                Ok(data) => {
                    all_wal.extend_from_slice(&data);
                    tracing::debug!("Restored segment {} ({} bytes)", segment_id, data.len());
                }
                Err(e) => {
                    tracing::warn!("Segment {} not available: {}", segment_id, e);
                    // Segments may not exist if they're before the branch point
                }
            }
        }

        // Trim to the exact target LSN
        let base_start = WalSegment::segment_start_lsn(base_segment_id);
        let byte_offset = (target_lsn.0 - base_start.0) as usize;
        if byte_offset < all_wal.len() {
            all_wal.truncate(byte_offset);
        }

        Ok(all_wal)
    }

    /// Generate S3 key for a WAL segment
    fn segment_key(&self, timeline_id: &str, segment_id: u64) -> String {
        format!("{}/{:016x}.wal.zst", timeline_id, segment_id)
    }

    /// List all archived segments for a timeline
    pub async fn list_segments(
        &self,
        timeline_id: &str,
    ) -> anyhow::Result<Vec<u64>> {
        let prefix = format!("{}/", timeline_id);

        let url = format!(
            "{}/{}?list-type=2&prefix={}",
            self.config.s3_endpoint,
            self.config.s3_bucket,
            prefix
        );

        let response = self.s3_client
            .get(&url)
            .basic_auth(
                &self.config.s3_access_key,
                Some(&self.config.s3_secret_key),
            )
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("S3 list failed: {}", response.status());
        }

        // Parse S3 list response (simplified)
        let body = response.text().await.unwrap_or_default();
        let mut segments = Vec::new();

        // Simple XML parsing for S3 list response
        for line in body.lines() {
            if line.contains("<Key>") {
                let key = line
                    .replace("<Key>", "")
                    .replace("</Key>", "")
                    .trim()
                    .to_string();

                if let Some(filename) = key.split('/').last() {
                    if let Some(hex_id) = filename.strip_suffix(".wal.zst") {
                        if let Ok(segment_id) = u64::from_str_radix(hex_id, 16) {
                            segments.push(segment_id);
                        }
                    }
                }
            }
        }

        segments.sort();
        Ok(segments)
    }
}

/// Trigger archival for a timeline's unarchived segments
pub async fn trigger_archive(
    axum::extract::Path(timeline_id): axum::extract::Path<String>,
    axum::extract::State(state): axum::extract::State<crate::WalState>,
) -> Result<axum::Json<serde_json::Value>, AppError> {
    let timelines = state.timelines.read().await;

    let timeline = timelines
        .get(&timeline_id)
        .ok_or_else(|| AppError::NotFound(format!("Timeline {} not found", timeline_id)))?;

    let mut archived_count = 0;

    for (_, segment) in &timeline.segments {
        if !segment.archived {
            match state.archiver.archive_segment(&timeline_id, segment).await {
                Ok(key) => {
                    tracing::info!("Archived segment: {}", key);
                    archived_count += 1;
                }
                Err(e) => {
                    tracing::error!("Failed to archive segment {}: {}", segment.segment_id, e);
                }
            }
        }
    }

    Ok(axum::Json(serde_json::json!({
        "timeline_id": timeline_id,
        "archived_segments": archived_count,
        "total_segments": timeline.segment_count(),
    })))
}
