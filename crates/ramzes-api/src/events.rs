use axum::Json;
use axum::extract::{Path, Query, State};
use ramzes_core::Severity;
use ramzes_store::{Delivery, EventFilter, EventRow, FailureRow, Journey, Outcome};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::ErrorBody;
use crate::{ApiError, AppState, Auth};

const DEFAULT_PAGE: i64 = 50;
const MAX_PAGE: i64 = 200;
const FAILURES_SHOWN: i64 = 200;

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OutcomeParam {
    Pending,
    Delivered,
    Failed,
    Unrouted,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct FeedQuery {
    /// Return events older than this id. Use `next_before` from the previous page.
    before: Option<String>,
    /// Source id.
    source: Option<String>,
    /// Exact topic, or a pattern where `*` matches any run of characters.
    topic: Option<String>,
    #[param(inline)]
    min_severity: Option<Severity>,
    /// One label as `key=value`.
    label: Option<String>,
    /// Case-insensitive text to find in the title.
    q: Option<String>,
    /// Received at or after this time, unix milliseconds.
    since: Option<i64>,
    /// Received at or before this time, unix milliseconds.
    until: Option<i64>,
    #[param(inline)]
    outcome: Option<OutcomeParam>,
    /// Page size, 1 to 200. Default 50.
    limit: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct EventPage {
    events: Vec<EventRow>,
    /// Pass as `before` to get the next page; absent on the last page.
    next_before: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct EventReplayed {
    /// Deliveries queued by the current routes.
    deliveries: usize,
}

#[derive(Deserialize, ToSchema)]
pub(crate) struct BulkReplay {
    delivery_ids: Vec<String>,
}

#[derive(Serialize, ToSchema)]
pub(crate) struct BulkReplayed {
    replayed: usize,
    /// Ids that did not exist.
    missing: Vec<String>,
}

/// A page of events, newest first.
#[utoipa::path(
    get, path = "/api/events", tag = "events", params(FeedQuery),
    responses((status = 200, body = EventPage), (status = 422, body = ErrorBody))
)]
pub(crate) async fn feed(
    State(state): State<AppState>,
    auth: Auth,
    Query(query): Query<FeedQuery>,
) -> Result<Json<EventPage>, ApiError> {
    let limit = query.limit.unwrap_or(DEFAULT_PAGE);
    if !(1..=MAX_PAGE).contains(&limit) {
        return Err(ApiError::invalid(format!("limit must be 1 to {MAX_PAGE}")));
    }
    let label = match query.label.as_deref() {
        None => None,
        Some(pair) => {
            let (key, value) = pair
                .split_once('=')
                .ok_or_else(|| ApiError::invalid("label must be given as key=value"))?;
            Some((key.to_owned(), value.to_owned()))
        }
    };
    let filter = EventFilter {
        before_id: query.before,
        source_id: query.source,
        topic: query.topic.filter(|t| !t.is_empty()),
        min_severity: query.min_severity,
        label,
        title_contains: query.q.filter(|q| !q.is_empty()),
        since_ms: query.since,
        until_ms: query.until,
        outcome: query.outcome.map(|o| match o {
            OutcomeParam::Pending => Outcome::Pending,
            OutcomeParam::Delivered => Outcome::Delivered,
            OutcomeParam::Failed => Outcome::Failed,
            OutcomeParam::Unrouted => Outcome::Unrouted,
        }),
        limit,
    };
    let events = state.store.list_events(&auth.workspace_id, &filter).await?;
    let full_page = i64::try_from(events.len()).is_ok_and(|n| n == limit);
    let next_before = if full_page {
        events.last().map(|row| row.event.id.clone())
    } else {
        None
    };
    Ok(Json(EventPage {
        events,
        next_before,
    }))
}

/// Everything that happened to one event: routes matched, deliveries and their attempts.
#[utoipa::path(
    get, path = "/api/events/{id}", tag = "events", params(("id" = String, Path)),
    responses((status = 200, body = Journey), (status = 404, body = ErrorBody))
)]
pub(crate) async fn journey(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<Json<Journey>, ApiError> {
    state
        .store
        .journey(&auth.workspace_id, &id)
        .await?
        .map(Json)
        .ok_or_else(ApiError::not_found)
}

/// Run an event through the current routes again.
#[utoipa::path(
    post, path = "/api/events/{id}/replay", tag = "events", params(("id" = String, Path)),
    responses((status = 200, body = EventReplayed), (status = 404, body = ErrorBody))
)]
pub(crate) async fn replay_event(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<Json<EventReplayed>, ApiError> {
    let deliveries = state.engine.replay_event(&auth.workspace_id, &id).await?;
    Ok(Json(EventReplayed { deliveries }))
}

/// Deliveries that failed for good, newest first.
#[utoipa::path(get, path = "/api/failures", tag = "failures", responses((status = 200, body = Vec<FailureRow>)))]
pub(crate) async fn failures(
    State(state): State<AppState>,
    auth: Auth,
) -> Result<Json<Vec<FailureRow>>, ApiError> {
    Ok(Json(
        state
            .store
            .list_failures(&auth.workspace_id, FAILURES_SHOWN)
            .await?,
    ))
}

/// Send a delivery again, to the same target.
#[utoipa::path(
    post, path = "/api/deliveries/{id}/replay", tag = "failures", params(("id" = String, Path)),
    responses((status = 200, body = Delivery), (status = 404, body = ErrorBody))
)]
pub(crate) async fn replay_delivery(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
) -> Result<Json<Delivery>, ApiError> {
    Ok(Json(
        state
            .engine
            .replay_delivery(&auth.workspace_id, &id)
            .await?,
    ))
}

/// Send several deliveries again.
#[utoipa::path(
    post, path = "/api/failures/replay", tag = "failures", request_body = BulkReplay,
    responses((status = 200, body = BulkReplayed))
)]
pub(crate) async fn replay_failures(
    State(state): State<AppState>,
    auth: Auth,
    Json(body): Json<BulkReplay>,
) -> Result<Json<BulkReplayed>, ApiError> {
    let mut result = BulkReplayed {
        replayed: 0,
        missing: Vec::new(),
    };
    for id in body.delivery_ids {
        match state.engine.replay_delivery(&auth.workspace_id, &id).await {
            Ok(_) => result.replayed += 1,
            Err(ramzes_engine::EngineError::NotFound) => result.missing.push(id),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(Json(result))
}
