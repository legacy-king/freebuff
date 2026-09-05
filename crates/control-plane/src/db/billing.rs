use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use freebuff_shared::{AppError, BillingAccount, InternalUsageEvent, UsageMeter};

// ── Billing accounts ───────────────────────────────────────────────────────

pub async fn get_billing_account(
    pool: &PgPool,
    org_id: Uuid,
) -> Result<Option<BillingAccount>, AppError> {
    let account = sqlx::query_as::<_, BillingAccount>(
        "SELECT * FROM billing_accounts WHERE org_id = $1",
    )
    .bind(org_id)
    .fetch_optional(pool)
    .await?;

    Ok(account)
}

pub async fn ensure_billing_account(pool: &PgPool, org_id: Uuid) -> Result<BillingAccount, AppError> {
    if let Some(existing) = get_billing_account(pool, org_id).await? {
        return Ok(existing);
    }

    let account = sqlx::query_as::<_, BillingAccount>(
        r#"
        INSERT INTO billing_accounts (org_id, plan, status)
        VALUES ($1, 'free', 'free')
        RETURNING *
        "#,
    )
    .bind(org_id)
    .fetch_one(pool)
    .await?;

    Ok(account)
}

pub async fn set_stripe_customer(
    pool: &PgPool,
    org_id: Uuid,
    customer_id: &str,
    billing_email: Option<&str>,
) -> Result<BillingAccount, AppError> {
    let account = sqlx::query_as::<_, BillingAccount>(
        r#"
        UPDATE billing_accounts
        SET stripe_customer_id = $2,
            billing_email = COALESCE($3, billing_email)
        WHERE org_id = $1
        RETURNING *
        "#,
    )
    .bind(org_id)
    .bind(customer_id)
    .bind(billing_email)
    .fetch_one(pool)
    .await?;

    Ok(account)
}

pub async fn update_subscription(
    pool: &PgPool,
    org_id: Uuid,
    subscription_id: &str,
    plan: &str,
    status: &str,
) -> Result<BillingAccount, AppError> {
    let account = sqlx::query_as::<_, BillingAccount>(
        r#"
        UPDATE billing_accounts
        SET stripe_subscription_id = $2, plan = $3, status = $4
        WHERE org_id = $1
        RETURNING *
        "#,
    )
    .bind(org_id)
    .bind(subscription_id)
    .bind(plan)
    .bind(status)
    .fetch_one(pool)
    .await?;

    Ok(account)
}

/// Resolve an org id from a Stripe customer id (used by webhook handlers).
pub async fn org_id_for_customer(pool: &PgPool, customer_id: &str) -> Result<Option<Uuid>, AppError> {
    let org_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT org_id FROM billing_accounts WHERE stripe_customer_id = $1",
    )
    .bind(customer_id)
    .fetch_optional(pool)
    .await?;

    Ok(org_id)
}

pub async fn set_billing_status(
    pool: &PgPool,
    org_id: Uuid,
    status: &str,
) -> Result<BillingAccount, AppError> {
    let account = sqlx::query_as::<_, BillingAccount>(
        "UPDATE billing_accounts SET status = $2 WHERE org_id = $1 RETURNING *",
    )
    .bind(org_id)
    .bind(status)
    .fetch_one(pool)
    .await?;

    Ok(account)
}

// ── Usage events ───────────────────────────────────────────────────────────

/// Insert raw usage events. Events carrying only a `project_id` have their
/// org resolved from the project. Rows are deduplicated by idempotency key.
pub async fn insert_usage_events(
    pool: &PgPool,
    events: &[InternalUsageEvent],
) -> Result<u64, AppError> {
    let mut tx = pool.begin().await?;

    let mut inserted = 0u64;
    for event in events {
        let org_id = match event.org_id {
            Some(org) => org,
            None => match event.project_id {
                Some(project_id) => {
                    let org: Option<Uuid> = sqlx::query_scalar(
                        "SELECT org_id FROM projects WHERE id = $1",
                    )
                    .bind(project_id)
                    .fetch_optional(&mut *tx)
                    .await?;
                    match org {
                        Some(org) => org,
                        None => {
                            tracing::warn!(
                                "Dropping usage event: unknown project {}",
                                project_id
                            );
                            continue;
                        }
                    }
                }
                None => {
                    tracing::warn!("Dropping usage event: no org_id or project_id");
                    continue;
                }
            },
        };

        let result = sqlx::query(
            r#"
            INSERT INTO usage_events (org_id, project_id, meter, value, occurred_at, idempotency_key)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (idempotency_key) DO NOTHING
            "#,
        )
        .bind(org_id)
        .bind(event.project_id)
        .bind(event.meter)
        .bind(event.value)
        .bind(event.occurred_at.unwrap_or_else(Utc::now))
        .bind(&event.idempotency_key)
        .execute(&mut *tx)
        .await?;

        inserted += result.rows_affected();
    }

    tx.commit().await?;
    Ok(inserted)
}

/// Aggregate pending raw events into per-day rows in `usage_daily`.
/// Returns the number of events rolled up.
pub async fn rollup_pending_usage(pool: &PgPool) -> Result<u64, AppError> {
    let mut tx = pool.begin().await?;

    let result = sqlx::query(
        r#"
        WITH pending AS (
            SELECT org_id, meter, date_trunc('day', occurred_at)::date AS day, SUM(value) AS v
            FROM usage_events
            WHERE rolled_up = false
            GROUP BY org_id, meter, day
        ),
        marked AS (
            UPDATE usage_events SET rolled_up = true WHERE rolled_up = false
        )
        INSERT INTO usage_daily (org_id, meter, window_start, value)
        SELECT org_id, meter, day, v FROM pending
        ON CONFLICT (org_id, meter, window_start)
        DO UPDATE SET value = usage_daily.value + EXCLUDED.value
        "#,
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(result.rows_affected())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PendingStripeSubmission {
    pub org_id: Uuid,
    pub meter: UsageMeter,
    pub month_start: NaiveDate,
    pub total: f64,
    pub stripe_customer_id: String,
}

/// Daily aggregates not yet submitted to Stripe, grouped by org/meter/month.
pub async fn pending_stripe_submissions(
    pool: &PgPool,
) -> Result<Vec<PendingStripeSubmission>, AppError> {
    let rows = sqlx::query_as::<_, PendingStripeSubmission>(
        r#"
        SELECT u.org_id,
               u.meter,
               date_trunc('month', u.window_start)::date AS month_start,
               SUM(u.value) AS total,
               ba.stripe_customer_id
        FROM usage_daily u
        JOIN billing_accounts ba ON ba.org_id = u.org_id
        WHERE u.submitted = false
          AND ba.stripe_customer_id IS NOT NULL
          AND ba.stripe_customer_id <> ''
        GROUP BY u.org_id, u.meter, month_start, ba.stripe_customer_id
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn mark_submissions_sent(
    pool: &PgPool,
    org_id: Uuid,
    meter: UsageMeter,
    month_start: NaiveDate,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE usage_daily
        SET submitted = true
        WHERE org_id = $1 AND meter = $2
          AND date_trunc('month', window_start)::date = $3
          AND submitted = false
        "#,
    )
    .bind(org_id)
    .bind(meter)
    .bind(month_start)
    .execute(pool)
    .await?;

    Ok(())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct UsageSummaryRow {
    pub meter: UsageMeter,
    pub day: NaiveDate,
    pub value: f64,
}

/// Per-day usage totals for an org within [start, end).
pub async fn usage_summary_rows(
    pool: &PgPool,
    org_id: Uuid,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<UsageSummaryRow>, AppError> {
    let rows = sqlx::query_as::<_, UsageSummaryRow>(
        r#"
        SELECT meter, date_trunc('day', occurred_at)::date AS day, SUM(value) AS value
        FROM usage_events
        WHERE org_id = $1 AND occurred_at >= $2 AND occurred_at < $3
        GROUP BY meter, day
        ORDER BY day
        "#,
    )
    .bind(org_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

// ── Stripe webhooks ────────────────────────────────────────────────────────

/// Record a processed webhook event; returns false if already processed.
pub async fn record_webhook_event(
    pool: &PgPool,
    stripe_event_id: &str,
    event_type: &str,
    payload: &serde_json::Value,
) -> Result<bool, AppError> {
    let result = sqlx::query(
        r#"
        INSERT INTO stripe_webhook_events (stripe_event_id, event_type, payload)
        VALUES ($1, $2, $3)
        ON CONFLICT (stripe_event_id) DO NOTHING
        "#,
    )
    .bind(stripe_event_id)
    .bind(event_type)
    .bind(payload)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}