use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Query, State};
use ramzes_core::Severity;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::ErrorBody;
use crate::{ApiError, AppState, Auth};

const HOUR_MS: i64 = 3_600_000;
const MAX_HOURS: i64 = 24 * 31;

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct OverviewQuery {
    /// How many hours back to look, 1 to 744. Default 24.
    hours: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct Bucket {
    /// Start of the bucket, unix milliseconds.
    start: i64,
    /// Events received in the bucket; the five severities below add up to it.
    events: i64,
    critical: i64,
    error: i64,
    warning: i64,
    info: i64,
    debug: i64,
    /// Deliveries created in the bucket that arrived.
    delivered: i64,
    /// Deliveries created in the bucket that failed for good.
    failed: i64,
}

/// The stretch of the same length just before the period.
#[derive(Serialize, ToSchema)]
pub(crate) struct Previous {
    events: i64,
    deliveries_delivered: i64,
    deliveries_failed: i64,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct Counted {
    name: String,
    events: i64,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct Failing {
    connection_id: String,
    /// Absent when the connection was deleted.
    name: Option<String>,
    failed: i64,
    delivered: i64,
    last_error: Option<String>,
    paused_until: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct OverviewView {
    /// Start of the period, unix milliseconds.
    since: i64,
    bucket_ms: i64,
    /// Events received in the period.
    events: i64,
    /// Events and delivery results per bucket, for buckets that have any, oldest first.
    buckets: Vec<Bucket>,
    previous: Previous,
    deliveries_pending: i64,
    deliveries_delivered: i64,
    deliveries_failed: i64,
    /// The sources that sent most.
    sources: Vec<Counted>,
    /// The topics with most events.
    topics: Vec<Counted>,
    /// Notifications collecting or held right now.
    waiting: i64,
    /// Notifications sent and neither acknowledged nor resolved.
    unacknowledged: i64,
    /// Connections with failed deliveries in the period or paused now, worst first.
    failing: Vec<Failing>,
}

/// Volume, delivery results, the noisiest sources and the failing connections of a period.
#[utoipa::path(
    get, path = "/api/overview", tag = "overview", params(OverviewQuery),
    responses((status = 200, body = OverviewView), (status = 422, body = ErrorBody))
)]
pub(crate) async fn overview(
    State(state): State<AppState>,
    auth: Auth,
    Query(query): Query<OverviewQuery>,
) -> Result<Json<OverviewView>, ApiError> {
    let hours = query.hours.unwrap_or(24);
    if !(1..=MAX_HOURS).contains(&hours) {
        return Err(ApiError::invalid(format!("hours must be 1 to {MAX_HOURS}")));
    }
    let ws = &auth.workspace_id;
    let now = state.clock.now_ms();
    // Hour buckets up to two days, day buckets beyond.
    let bucket_ms = if hours <= 48 { HOUR_MS } else { 24 * HOUR_MS };
    let since = (now - hours * HOUR_MS) / bucket_ms * bucket_ms;
    let previous_since = since - hours * HOUR_MS;
    let found = state
        .store
        .overview(ws, since, bucket_ms, previous_since)
        .await?;
    let health = state.store.connection_health(ws, since).await?;
    let status = state.store.connection_status(ws).await?;
    let connections = state.store.list_connections(ws).await?;
    let name_of = |id: &str| {
        connections
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
    };

    let mut failing: Vec<Failing> = health
        .into_iter()
        .map(|h| Failing {
            name: name_of(&h.connection_id),
            paused_until: status
                .get(&h.connection_id)
                .and_then(|s| s.paused_until)
                .filter(|until| *until > now),
            connection_id: h.connection_id,
            failed: h.failed,
            delivered: h.delivered,
            last_error: h.last_error,
        })
        .filter(|f| f.failed > 0 || f.paused_until.is_some())
        .collect();
    failing.sort_by(|a, b| b.failed.cmp(&a.failed).then_with(|| a.name.cmp(&b.name)));
    let counted = |rows: Vec<(String, i64)>| {
        rows.into_iter()
            .map(|(name, events)| Counted { name, events })
            .collect()
    };
    let mut buckets: BTreeMap<i64, Bucket> = BTreeMap::new();
    let empty = |start| Bucket {
        start,
        events: 0,
        critical: 0,
        error: 0,
        warning: 0,
        info: 0,
        debug: 0,
        delivered: 0,
        failed: 0,
    };
    for (start, severity, count) in found.severity_buckets {
        let bucket = buckets.entry(start).or_insert_with(|| empty(start));
        bucket.events += count;
        match Severity::from_i64(severity) {
            Some(Severity::Critical) => bucket.critical += count,
            Some(Severity::Error) => bucket.error += count,
            Some(Severity::Warning) => bucket.warning += count,
            Some(Severity::Debug) => bucket.debug += count,
            Some(Severity::Info) | None => bucket.info += count,
        }
    }
    for (start, delivered, failed) in found.delivery_buckets {
        let bucket = buckets.entry(start).or_insert_with(|| empty(start));
        bucket.delivered = delivered;
        bucket.failed = failed;
    }
    let (previous_events, previous_delivered, previous_failed) = found.previous;
    Ok(Json(OverviewView {
        since,
        bucket_ms,
        events: found.events,
        buckets: buckets.into_values().collect(),
        previous: Previous {
            events: previous_events,
            deliveries_delivered: previous_delivered,
            deliveries_failed: previous_failed,
        },
        deliveries_pending: found.deliveries_pending,
        deliveries_delivered: found.deliveries_delivered,
        deliveries_failed: found.deliveries_failed,
        sources: counted(found.sources),
        topics: counted(found.topics),
        waiting: found.waiting,
        unacknowledged: found.unacknowledged,
        failing,
    }))
}
