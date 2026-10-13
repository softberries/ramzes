use serde::Serialize;
use sqlx::AssertSqlSafe;

use crate::{Result, Store, with_db};

/// What happened since a point in time, for the overview screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Overview {
    /// Events received in the period.
    pub events: i64,
    /// `(start of bucket, events)` for buckets that have events, oldest first.
    pub buckets: Vec<(i64, i64)>,
    /// `(start of bucket, severity, events)`, oldest first; severity as [`ramzes_core::Severity::as_i64`].
    pub severity_buckets: Vec<(i64, i64, i64)>,
    /// `(start of bucket, delivered, failed)` for deliveries created in the bucket, oldest first.
    pub delivery_buckets: Vec<(i64, i64, i64)>,
    /// `(events, deliveries delivered, deliveries failed)` of the stretch before the period, to
    /// compare it with.
    pub previous: (i64, i64, i64),
    pub deliveries_pending: i64,
    pub deliveries_delivered: i64,
    pub deliveries_failed: i64,
    /// The sources that sent most, `(name, events)`.
    pub sources: Vec<(String, i64)>,
    /// The topics with most events, `(topic, events)`.
    pub topics: Vec<(String, i64)>,
    /// Notifications collecting or held right now.
    pub waiting: i64,
    /// Notifications sent and neither acknowledged nor resolved.
    pub unacknowledged: i64,
}

impl Store {
    /// Counts for events received and deliveries created at or after `since_ms`. The stretch
    /// from `previous_since_ms` up to `since_ms` is counted too, for comparison.
    pub async fn overview(
        &self,
        workspace_id: &str,
        since_ms: i64,
        bucket_ms: i64,
        previous_since_ms: i64,
    ) -> Result<Overview> {
        let q = |text: &str| self.sql(text).into_owned();
        let buckets = q(
            "SELECT (received_at / $3) * $3 AS bucket, COUNT(*) FROM events \
             WHERE workspace_id = $1 AND received_at >= $2 GROUP BY bucket ORDER BY bucket",
        );
        let severity_buckets = q(
            "SELECT (received_at / $3) * $3 AS bucket, severity, COUNT(*) FROM events \
             WHERE workspace_id = $1 AND received_at >= $2 GROUP BY bucket, severity ORDER BY bucket, severity",
        );
        let delivery_buckets = q(
            "SELECT (created_at / $3) * $3 AS bucket, COUNT(CASE WHEN state = 'delivered' THEN 1 END), \
             COUNT(CASE WHEN state IN ('failed', 'dead') THEN 1 END) \
             FROM deliveries WHERE workspace_id = $1 AND created_at >= $2 GROUP BY bucket ORDER BY bucket",
        );
        let previous_events = q(
            "SELECT COUNT(*) FROM events WHERE workspace_id = $1 AND received_at >= $2 AND received_at < $3",
        );
        let previous_deliveries = q(
            "SELECT COUNT(CASE WHEN state = 'delivered' THEN 1 END), COUNT(CASE WHEN state IN ('failed', 'dead') THEN 1 END) \
             FROM deliveries WHERE workspace_id = $1 AND created_at >= $2 AND created_at < $3",
        );
        let deliveries = q(
            "SELECT COUNT(CASE WHEN state IN ('pending', 'in_progress') THEN 1 END), \
             COUNT(CASE WHEN state = 'delivered' THEN 1 END), \
             COUNT(CASE WHEN state IN ('failed', 'dead') THEN 1 END) \
             FROM deliveries WHERE workspace_id = $1 AND created_at >= $2",
        );
        let sources = q(
            "SELECT COALESCE(s.name, 'deleted source') AS name, COUNT(*) AS c FROM events e \
             LEFT JOIN sources s ON s.id = e.source_id WHERE e.workspace_id = $1 AND e.received_at >= $2 \
             GROUP BY s.name ORDER BY c DESC, name LIMIT 5",
        );
        let topics = q(
            "SELECT topic, COUNT(*) AS c FROM events WHERE workspace_id = $1 AND received_at >= $2 \
             GROUP BY topic ORDER BY c DESC, topic LIMIT 5",
        );
        let open = q(
            "SELECT COUNT(CASE WHEN state IN ('grouping', 'held') THEN 1 END), COUNT(CASE WHEN state = 'sent' THEN 1 END) \
             FROM notifications WHERE workspace_id = $1 AND state IN ('grouping', 'held', 'sent')",
        );
        with_db!(self, |pool| {
            let buckets: Vec<(i64, i64)> = sqlx::query_as(AssertSqlSafe(buckets.as_str()))
                .bind(workspace_id)
                .bind(since_ms)
                .bind(bucket_ms.max(1))
                .fetch_all(pool)
                .await?;
            let severity_buckets: Vec<(i64, i64, i64)> =
                sqlx::query_as(AssertSqlSafe(severity_buckets.as_str()))
                    .bind(workspace_id)
                    .bind(since_ms)
                    .bind(bucket_ms.max(1))
                    .fetch_all(pool)
                    .await?;
            let delivery_buckets: Vec<(i64, i64, i64)> =
                sqlx::query_as(AssertSqlSafe(delivery_buckets.as_str()))
                    .bind(workspace_id)
                    .bind(since_ms)
                    .bind(bucket_ms.max(1))
                    .fetch_all(pool)
                    .await?;
            let (previous_events,): (i64,) =
                sqlx::query_as(AssertSqlSafe(previous_events.as_str()))
                    .bind(workspace_id)
                    .bind(previous_since_ms)
                    .bind(since_ms)
                    .fetch_one(pool)
                    .await?;
            let (previous_delivered, previous_failed): (i64, i64) =
                sqlx::query_as(AssertSqlSafe(previous_deliveries.as_str()))
                    .bind(workspace_id)
                    .bind(previous_since_ms)
                    .bind(since_ms)
                    .fetch_one(pool)
                    .await?;
            let (pending, delivered, failed): (i64, i64, i64) =
                sqlx::query_as(AssertSqlSafe(deliveries.as_str()))
                    .bind(workspace_id)
                    .bind(since_ms)
                    .fetch_one(pool)
                    .await?;
            let sources: Vec<(String, i64)> = sqlx::query_as(AssertSqlSafe(sources.as_str()))
                .bind(workspace_id)
                .bind(since_ms)
                .fetch_all(pool)
                .await?;
            let topics: Vec<(String, i64)> = sqlx::query_as(AssertSqlSafe(topics.as_str()))
                .bind(workspace_id)
                .bind(since_ms)
                .fetch_all(pool)
                .await?;
            let (waiting, unacknowledged): (i64, i64) =
                sqlx::query_as(AssertSqlSafe(open.as_str()))
                    .bind(workspace_id)
                    .fetch_one(pool)
                    .await?;
            Ok(Overview {
                events: buckets.iter().map(|(_, count)| count).sum(),
                buckets,
                severity_buckets,
                delivery_buckets,
                previous: (previous_events, previous_delivered, previous_failed),
                deliveries_pending: pending,
                deliveries_delivered: delivered,
                deliveries_failed: failed,
                sources,
                topics,
                waiting,
                unacknowledged,
            })
        })
    }
}
