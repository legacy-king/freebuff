use chrono::{NaiveDate, Utc};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

use crate::AppState;
use freebuff_shared::{AppError, Config, InternalUsageEvent, UsageMeter};

/// Record raw usage events (from the gateway, storage sampler, etc.).
pub async fn record_events(
    state: &AppState,
    events: &[InternalUsageEvent],
) -> Result<u64, AppError> {
    crate::db::billing::insert_usage_events(&state.db, events).await
}

/// Accrue compute-hours for every running endpoint since its last accrual.
///
/// Endpoints created while running start accruing from the moment they are
/// first observed. Wall-clock elapsed time is multiplied by the endpoint's
/// compute size factor (micro = 0.25x, small = 1x, medium = 4x, large = 16x).
pub async fn run_compute_accrual(pool: &sqlx::PgPool) -> Result<u64, AppError> {
    let now = Utc::now();
    let endpoints = crate::db::compute::running_endpoints(pool).await?;

    let mut events: Vec<InternalUsageEvent> = Vec::new();
    let mut initialized: Vec<(Uuid, chrono::DateTime<Utc>)> = Vec::new();

    for endpoint in endpoints {
        match endpoint.last_accrued_at {
            None => {
                // First observation: seed the accrual clock without billing.
                initialized.push((endpoint.id, now));
            }
            Some(last) => {
                let secs = (now - last).num_seconds().max(0);
                if secs >= 1 {
                    let hours = (secs as f64 / 3600.0) * endpoint.compute_size.compute_factor();
                    events.push(InternalUsageEvent {
                        org_id: Some(endpoint.org_id),
                        project_id: Some(endpoint.project_id),
                        meter: UsageMeter::ComputeHours,
                        value: hours,
                        occurred_at: Some(now),
                        // Deterministic per endpoint+window: safe to retry.
                        idempotency_key: Some(format!("compute-{}-{}", endpoint.id, last.timestamp())),
                    });
                    initialized.push((endpoint.id, now));
                }
            }
        }
    }

    let inserted = crate::db::billing::insert_usage_events(pool, &events).await?;
    for (endpoint_id, timestamp) in initialized {
        crate::db::compute::set_last_accrued_at(pool, endpoint_id, timestamp).await?;
    }

    Ok(inserted)
}

/// Sample live database size (`pg_database_size`) for every active project
/// and record it as storage-gb usage.
pub async fn sample_storage(pool: &sqlx::PgPool, config: &Config) -> Result<u64, AppError> {
    let projects = crate::db::projects::active_projects(pool).await?;
    let now = Utc::now();
    let mut events: Vec<InternalUsageEvent> = Vec::new();

    for project in projects {
        let host = project.database_host.as_deref().unwrap_or("localhost");
        let port = project.database_port.unwrap_or(5432);
        let database = project
            .database_name
            .as_deref()
            .unwrap_or("postgres");
        let url = format!(
            "postgres://{}:{}@{}:{}/{}?sslmode=disable",
            config.pg_monitor_user, config.pg_monitor_password, host, port, database
        );

        match PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(std::time::Duration::from_secs(5))
            .connect(&url)
            .await
        {
            Ok(db_pool) => {
                let bytes: Result<i64, _> =
                    sqlx::query_scalar("SELECT pg_database_size(current_database())")
                        .fetch_one(&db_pool)
                        .await;
                db_pool.close().await;
                match bytes {
                    Ok(bytes) if bytes > 0 => {
                        let gb = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
                        events.push(InternalUsageEvent {
                            org_id: Some(project.org_id),
                            project_id: Some(project.id),
                            meter: UsageMeter::StorageGb,
                            value: gb,
                            occurred_at: Some(now),
                            idempotency_key: Some(format!("storage-{}-{}", project.id, now.timestamp())),
                        });
                    }
                    Ok(_) => {
                        tracing::debug!("Database {} reports zero size", database);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to measure {}: {}", database, e);
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Cannot connect to project database {} at {}:{} — skipping storage sample: {}",
                    database,
                    host,
                    port,
                    e
                );
            }
        }
    }

    crate::db::billing::insert_usage_events(pool, &events).await
}

/// Deterministic idempotency identifier for a Stripe meter event submission.
fn submission_identifier(org_id: Uuid, meter: UsageMeter, month: NaiveDate) -> String {
    let input = format!("{}-{}-{}", org_id, meter, month);
    let digest = Sha256::digest(input.as_bytes());
    format!("{:x}", digest)
}

/// Roll up pending usage events into daily aggregates, then report them to
/// Stripe Billing Meters. Returns the number of meter events submitted.
pub async fn submit_usage_to_stripe(state: &AppState) -> Result<u64, AppError> {
    crate::db::billing::rollup_pending_usage(&state.db).await?;

    if !state.config.stripe_enabled() {
        return Ok(0);
    }

    let pending = crate::db::billing::pending_stripe_submissions(&state.db).await?;
    let mut submitted = 0u64;

    for row in pending {
        let event_name = state.config.meter_event_name(row.meter);
        let identifier = submission_identifier(row.org_id, row.meter, row.month_start);
        match crate::services::stripe_service::submit_meter_event(
            &state.config,
            &state.http,
            &event_name,
            &identifier,
            &row.stripe_customer_id,
            row.total,
        )
        .await
        {
            Ok(()) => {
                crate::db::billing::mark_submissions_sent(
                    &state.db,
                    row.org_id,
                    row.meter,
                    row.month_start,
                )
                .await?;
                submitted += 1;
                tracing::info!(
                    "Reported {} of {} to Stripe meter {} for org {}",
                    row.total,
                    row.meter,
                    event_name,
                    row.org_id
                );
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to report {} to Stripe for org {}: {}",
                    row.meter,
                    row.org_id,
                    e
                );
            }
        }
    }

    Ok(submitted)
}

/// Background loop: accrue compute hours for running endpoints every 60s.
pub async fn compute_accrual_loop(state: AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    loop {
        interval.tick().await;
        match run_compute_accrual(&state.db).await {
            Ok(0) => {}
            Ok(n) => tracing::debug!("Accrued {} compute usage events", n),
            Err(e) => tracing::warn!("Compute accrual failed: {}", e),
        }
    }
}

/// Background loop: roll up usage and report it to Stripe every 60s.
pub async fn usage_report_loop(state: AppState) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    loop {
        interval.tick().await;
        match submit_usage_to_stripe(&state).await {
            Ok(0) => {}
            Ok(n) => tracing::debug!("Submitted {} usage records to Stripe", n),
            Err(e) => tracing::warn!("Usage reporting to Stripe failed: {}", e),
        }
    }
}

/// Background loop: sample database storage at the configured interval.
pub async fn storage_sampler_loop(state: AppState) {
    let secs = state.config.storage_sample_secs.max(60);
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(secs));
    loop {
        interval.tick().await;
        match sample_storage(&state.db, &state.config).await {
            Ok(0) => {}
            Ok(n) => tracing::debug!("Recorded {} storage usage events", n),
            Err(e) => tracing::warn!("Storage sampling failed: {}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn submission_identifier_is_stable_and_compact() {
        let org = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let month = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();

        let a = submission_identifier(org, UsageMeter::ComputeHours, month);
        let b = submission_identifier(org, UsageMeter::ComputeHours, month);
        let c = submission_identifier(org, UsageMeter::ApiCalls, month);

        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn compute_hours_conversion_uses_size_factor() {
        // 1 hour of small = 1.0 compute hour; micro = 0.25; large = 16.
        let secs = 3600.0;
        let small = secs / 3600.0 * freebuff_shared::ComputeSize::Small.compute_factor();
        let micro = secs / 3600.0 * freebuff_shared::ComputeSize::Micro.compute_factor();
        let large = secs / 3600.0 * freebuff_shared::ComputeSize::Large.compute_factor();
        assert!((small - 1.0).abs() < 1e-9);
        assert!((micro - 0.25).abs() < 1e-9);
        assert!((large - 16.0).abs() < 1e-9);
    }

    #[test]
    fn old_timestamps_dont_accrue_negative_time() {
        // Simulates the accrual math: elapsed must never be negative.
        let now = Utc::now();
        let last = now + Duration::minutes(5); // clock moved backwards
        let secs = (now - last).num_seconds().max(0);
        assert_eq!(secs, 0);
    }
}